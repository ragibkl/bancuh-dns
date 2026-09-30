use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    string::FromUtf8Error,
    sync::atomic::{AtomicBool, Ordering},
};

use rand::{Rng, distr::Alphanumeric};
use rocksdb::{DBWithThreadMode, MultiThreaded, Options, WriteBatch};
use thiserror::Error;

pub type DB = DBWithThreadMode<MultiThreaded>;

/// Entries per RocksDB write batch. Batching amortises the per-write overhead across the
/// millions of domains a compile writes; the cap keeps any single batch small in memory.
const WRITE_BATCH_SIZE: usize = 10_000;

/// Version of the on-disk layout and key format. A saved generation written with a
/// different version is ignored and rebuilt. Bump it whenever a change to how domains are
/// normalised or stored would make an older generation answer differently.
const FORMAT_VERSION: u32 = 1;

/// Name of the file that records which generation is current.
const CURRENT_FILE: &str = "current";
const CURRENT_TMP_FILE: &str = "current.tmp";
const LOCK_FILE: &str = "lock";
const GENERATION_PREFIX: &str = "gen-";
/// Directories from builds before generations were kept across restarts.
const LEGACY_PREFIX: &str = "db-";

fn rand_string() -> String {
    rand::rng()
        .sample_iter(Alphanumeric)
        .take(10)
        .map(char::from)
        .collect()
}

fn generation_name() -> String {
    format!(
        "{GENERATION_PREFIX}{}-{}",
        chrono::Utc::now().timestamp(),
        rand_string()
    )
}

fn normalize_name(name: &str) -> String {
    if name.ends_with('.') {
        name.to_string()
    } else {
        format!("{name}.")
    }
}

#[derive(Debug, Error)]
pub enum DBError {
    #[error(transparent)]
    RocksDB(#[from] rocksdb::Error),

    #[error(transparent)]
    FromUtf8(#[from] FromUtf8Error),

    #[error(transparent)]
    Io(#[from] io::Error),

    #[error("db dir {0} is in use by another process")]
    Locked(PathBuf),

    #[error("saved generation pointer is invalid: {0:?}")]
    BadPointer(String),

    #[error("saved generation has format version {0}, expected {FORMAT_VERSION}")]
    FormatVersion(u32),
}

#[derive(Debug)]
pub struct DomainStore {
    db: Option<DB>,
}

impl DomainStore {
    fn create(path: &Path) -> Result<Self, DBError> {
        let db = DB::open_default(path)?;

        Ok(Self { db: Some(db) })
    }

    /// Open a store that must already exist.
    fn open(path: &Path) -> Result<Self, DBError> {
        let mut opts = Options::default();
        opts.create_if_missing(false);
        let db = DB::open(&opts, path)?;

        Ok(Self { db: Some(db) })
    }

    fn flush(&self) -> Result<(), DBError> {
        if let Some(db) = &self.db {
            db.flush()?;
        }

        Ok(())
    }

    fn close(&mut self) {
        if let Some(db) = self.db.take() {
            db.cancel_all_background_work(true);
        }
    }

    /// Write many domains in batches.
    ///
    /// Synchronous and potentially long-running: call this from a blocking context, not
    /// directly on an async worker thread.
    pub fn put_all<I, S>(&self, domains: I) -> Result<(), DBError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let Some(db) = &self.db else {
            return Ok(());
        };

        let mut batch = WriteBatch::default();
        for domain in domains {
            batch.put(normalize_name(domain.as_ref()), "true");
            if batch.len() >= WRITE_BATCH_SIZE {
                db.write(std::mem::take(&mut batch))?;
            }
        }
        if !batch.is_empty() {
            db.write(batch)?;
        }

        Ok(())
    }

    /// Write many domain -> alias pairs in batches. See [`DomainStore::put_all`].
    pub fn put_aliases_all<I, S>(&self, aliases: I) -> Result<(), DBError>
    where
        I: IntoIterator<Item = (S, S)>,
        S: AsRef<str>,
    {
        let Some(db) = &self.db else {
            return Ok(());
        };

        let mut batch = WriteBatch::default();
        for (domain, alias) in aliases {
            batch.put(
                normalize_name(domain.as_ref()),
                normalize_name(alias.as_ref()),
            );
            if batch.len() >= WRITE_BATCH_SIZE {
                db.write(std::mem::take(&mut batch))?;
            }
        }
        if !batch.is_empty() {
            db.write(batch)?;
        }

        Ok(())
    }

    pub fn get(&self, domain: &str) -> Result<Option<String>, DBError> {
        if let Some(db) = &self.db {
            let parts: Vec<&str> = domain.split('.').filter(|s| !s.is_empty()).collect();

            let mut keys: Vec<String> = vec![domain.to_string()];
            for i in 1..parts.len() {
                let star_key = format!("*.{}.", parts[i..parts.len()].join("."));
                keys.push(star_key);
            }

            for key in keys.iter() {
                if let Some(s) = db.get(key)? {
                    return Ok(Some(String::from_utf8(s)?));
                }
            }
        }

        Ok(None)
    }

    pub fn contains(&self, domain: &str) -> Result<bool, DBError> {
        self.get(domain).map(|o| o.is_some())
    }
}

/// One compiled blocklist: a generation directory holding the three stores.
///
/// A generation is deleted from disk when it is dropped, unless it is the committed one.
/// A new generation starts out discardable, so a compile that fails or is cancelled
/// leaves nothing behind; [`DbDir::commit`] keeps it, and the engine marks the generation
/// it replaces discardable again. The current generation is therefore kept on shutdown
/// and loaded again on the next start.
#[derive(Debug)]
pub struct AdblockDB {
    pub blacklist: DomainStore,
    pub whitelist: DomainStore,
    pub rewrites: DomainStore,
    dir: PathBuf,
    discard: AtomicBool,
}

impl AdblockDB {
    fn create(dir: PathBuf) -> Result<Self, DBError> {
        let db = Self {
            blacklist: DomainStore::create(&dir.join("blacklist"))?,
            whitelist: DomainStore::create(&dir.join("whitelist"))?,
            rewrites: DomainStore::create(&dir.join("rewrites"))?,
            dir,
            discard: AtomicBool::new(true),
        };

        Ok(db)
    }

    fn open(dir: PathBuf) -> Result<Self, DBError> {
        let db = Self {
            blacklist: DomainStore::open(&dir.join("blacklist"))?,
            whitelist: DomainStore::open(&dir.join("whitelist"))?,
            rewrites: DomainStore::open(&dir.join("rewrites"))?,
            dir,
            discard: AtomicBool::new(false),
        };

        Ok(db)
    }

    fn flush(&self) -> Result<(), DBError> {
        self.blacklist.flush()?;
        self.whitelist.flush()?;
        self.rewrites.flush()?;

        Ok(())
    }

    /// The generation's directory name, e.g. `gen-1790000000-AbCdEfGhIj`.
    pub fn name(&self) -> String {
        self.dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    /// When this generation was compiled, from the timestamp in its name.
    pub fn compiled_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        let name = self.name();
        let ts = name.strip_prefix(GENERATION_PREFIX)?.split('-').next()?;
        chrono::DateTime::from_timestamp(ts.parse().ok()?, 0)
    }

    /// Delete this generation from disk once the last reference to it is dropped.
    pub fn set_discard(&self) {
        self.discard.store(true, Ordering::SeqCst);
    }
}

impl Drop for AdblockDB {
    fn drop(&mut self) {
        self.blacklist.close();
        self.whitelist.close();
        self.rewrites.close();

        if self.discard.load(Ordering::SeqCst) {
            let path = self.dir.to_string_lossy().to_string();
            tracing::info!("Destroying db: {path}");
            let res = fs::remove_dir_all(&self.dir);
            tracing::info!("Destroying db: {path}. DONE: {res:?}");
        }
    }
}

/// The directory that holds compiled generations across restarts.
///
/// ```text
/// <root>/lock                 held for as long as this process runs
/// <root>/current              "<format version> <generation name>"
/// <root>/gen-<unix time>-<id>/{blacklist,whitelist,rewrites}
/// ```
///
/// `current` is only ever replaced by an atomic rename, after the generation it names has
/// been flushed, so it always names a complete generation or is absent. Anything else in
/// the directory is left over from a crash or a replaced generation, and is removed by
/// [`DbDir::clean`] at startup.
#[derive(Debug)]
pub struct DbDir {
    root: PathBuf,
    _lock: File,
}

impl DbDir {
    /// Create `root` if needed and take an exclusive lock on it.
    ///
    /// The lock matters because startup deletes every generation it does not load: a
    /// second process sharing the directory would have its live generation removed.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, DBError> {
        let root = root.into();
        fs::create_dir_all(&root)?;

        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(root.join(LOCK_FILE))?;
        lock.try_lock().map_err(|_| DBError::Locked(root.clone()))?;

        Ok(Self { root, _lock: lock })
    }

    /// Open the generation named by `current`, if there is a usable one.
    pub fn load_current(&self) -> Result<Option<AdblockDB>, DBError> {
        let pointer = match fs::read_to_string(self.root.join(CURRENT_FILE)) {
            Ok(s) => s,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };

        let (version, name) = pointer
            .trim()
            .split_once(' ')
            .ok_or_else(|| DBError::BadPointer(pointer.clone()))?;
        let version: u32 = version
            .parse()
            .map_err(|_| DBError::BadPointer(pointer.clone()))?;
        if version != FORMAT_VERSION {
            return Err(DBError::FormatVersion(version));
        }
        if !name.starts_with(GENERATION_PREFIX) || name.contains('/') {
            return Err(DBError::BadPointer(pointer.clone()));
        }

        AdblockDB::open(self.root.join(name)).map(Some)
    }

    /// Remove every generation except `keep`, and any leftovers from older builds or an
    /// interrupted commit. Without a generation to keep, `current` goes too.
    pub fn clean(&self, keep: Option<&AdblockDB>) -> Result<(), DBError> {
        let keep = keep.map(|db| db.name());
        if keep.is_none() {
            remove_file_if_exists(&self.root.join(CURRENT_FILE))?;
        }
        remove_file_if_exists(&self.root.join(CURRENT_TMP_FILE))?;

        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            let stale = name.starts_with(GENERATION_PREFIX) || name.starts_with(LEGACY_PREFIX);
            if !stale || keep.as_deref() == Some(name.as_str()) {
                continue;
            }

            tracing::info!("Removing stale db: {name}");
            fs::remove_dir_all(entry.path())?;
        }

        Ok(())
    }

    /// Create an empty generation to compile into. It is deleted when dropped unless it
    /// is committed.
    pub fn new_generation(&self) -> Result<AdblockDB, DBError> {
        AdblockDB::create(self.root.join(generation_name()))
    }

    /// Flush `db` and make it the generation loaded on the next start.
    ///
    /// Synchronous: call this from a blocking context.
    pub fn commit(&self, db: &AdblockDB) -> Result<(), DBError> {
        db.flush()?;

        let tmp = self.root.join(CURRENT_TMP_FILE);
        let mut file = File::create(&tmp)?;
        writeln!(file, "{FORMAT_VERSION} {}", db.name())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, self.root.join(CURRENT_FILE))?;

        // From here on the pointer names this generation, so it must not be deleted even
        // if the directory sync below fails.
        db.discard.store(false, Ordering::SeqCst);

        // Persist the rename itself. Without this a power loss could bring back the old
        // pointer, which is harmless: that generation is only deleted once replaced.
        if let Err(err) = File::open(&self.root).and_then(|d| d.sync_all()) {
            tracing::warn!("Syncing db dir after commit failed: {err}");
        }

        Ok(())
    }
}

fn remove_file_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!("bancuh-db-test-{}", rand_string()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn compile(dir: &DbDir, domains: &[&str]) -> AdblockDB {
        let db = dir.new_generation().unwrap();
        db.blacklist.put_all(domains.iter().copied()).unwrap();
        db
    }

    fn generations(root: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with(GENERATION_PREFIX))
            .collect();
        names.sort();
        names
    }

    #[test]
    fn committed_generation_survives_restart() {
        let root = temp_root();
        {
            let dir = DbDir::open(&root).unwrap();
            let db = compile(&dir, &["blocked.example"]);
            dir.commit(&db).unwrap();
        }

        let dir = DbDir::open(&root).unwrap();
        let db = dir.load_current().unwrap().expect("a saved generation");
        assert!(db.blacklist.contains("blocked.example.").unwrap());
        assert!(!db.blacklist.contains("allowed.example.").unwrap());
        assert!(db.compiled_at().is_some());

        drop(db);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn uncommitted_generation_is_removed_on_drop() {
        let root = temp_root();
        let dir = DbDir::open(&root).unwrap();
        drop(compile(&dir, &["blocked.example"]));

        assert!(generations(&root).is_empty());
        assert!(dir.load_current().unwrap().is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replaced_generation_is_removed_and_pointer_moves() {
        let root = temp_root();
        let dir = DbDir::open(&root).unwrap();

        let old = compile(&dir, &["old.example"]);
        dir.commit(&old).unwrap();
        let new = compile(&dir, &["new.example"]);
        dir.commit(&new).unwrap();
        old.set_discard();
        drop(old);

        assert_eq!(generations(&root), vec![new.name()]);
        drop(new);
        drop(dir);

        let dir = DbDir::open(&root).unwrap();
        let db = dir.load_current().unwrap().unwrap();
        assert!(db.blacklist.contains("new.example.").unwrap());
        drop(db);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clean_keeps_only_the_loaded_generation() {
        let root = temp_root();
        let dir = DbDir::open(&root).unwrap();
        let current = compile(&dir, &["a.example"]);
        dir.commit(&current).unwrap();

        // leftovers: an interrupted compile, an older build's db and a half-written pointer
        let partial = compile(&dir, &["b.example"]);
        let partial_name = partial.name();
        std::mem::forget(partial);
        fs::create_dir_all(root.join("db-legacy01")).unwrap();
        fs::write(root.join(CURRENT_TMP_FILE), "garbage").unwrap();
        fs::write(root.join("unrelated"), "kept").unwrap();
        assert!(generations(&root).contains(&partial_name));

        dir.clean(Some(&current)).unwrap();

        assert_eq!(generations(&root), vec![current.name()]);
        assert!(!root.join("db-legacy01").exists());
        assert!(!root.join(CURRENT_TMP_FILE).exists());
        assert!(root.join(CURRENT_FILE).exists());
        assert!(root.join("unrelated").exists());
        drop(current);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clean_without_generation_removes_pointer() {
        let root = temp_root();
        let dir = DbDir::open(&root).unwrap();
        let db = compile(&dir, &["a.example"]);
        dir.commit(&db).unwrap();
        std::mem::forget(db);

        dir.clean(None).unwrap();

        assert!(generations(&root).is_empty());
        assert!(dir.load_current().unwrap().is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pointer_to_missing_or_foreign_generation_is_an_error() {
        let root = temp_root();
        let dir = DbDir::open(&root).unwrap();

        fs::write(root.join(CURRENT_FILE), "1 gen-1-missing\n").unwrap();
        assert!(dir.load_current().is_err());

        fs::write(root.join(CURRENT_FILE), "99 gen-1-missing\n").unwrap();
        assert!(matches!(
            dir.load_current(),
            Err(DBError::FormatVersion(99))
        ));

        fs::write(root.join(CURRENT_FILE), "1 ../etc\n").unwrap();
        assert!(matches!(dir.load_current(), Err(DBError::BadPointer(_))));

        fs::write(root.join(CURRENT_FILE), "nonsense").unwrap();
        assert!(matches!(dir.load_current(), Err(DBError::BadPointer(_))));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn second_process_cannot_take_the_dir() {
        let root = temp_root();
        let _dir = DbDir::open(&root).unwrap();

        assert!(matches!(DbDir::open(&root), Err(DBError::Locked(_))));
        fs::remove_dir_all(root).unwrap();
    }
}
