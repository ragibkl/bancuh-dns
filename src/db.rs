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
const GENERATION_ID_LEN: usize = 10;

fn rand_string() -> String {
    rand::rng()
        .sample_iter(Alphanumeric)
        .take(GENERATION_ID_LEN)
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

/// Whether `name` is exactly what [`generation_name`] produces: `gen-<unix time>-<id>`.
/// Startup cleanup deletes only entries matching this, so a DB_DIR shared with anything
/// else loses nothing but its generations.
fn is_generation_name(name: &str) -> bool {
    let Some((ts, id)) = name
        .strip_prefix(GENERATION_PREFIX)
        .and_then(|rest| rest.split_once('-'))
    else {
        return false;
    };

    !ts.is_empty()
        && ts.bytes().all(|b| b.is_ascii_digit())
        && id.len() == GENERATION_ID_LEN
        && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// fsync a directory, so the entries created or renamed in it survive a power loss.
fn sync_dir(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
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

    #[error("saved generation {0} does not exist")]
    MissingGeneration(String),
}

impl DBError {
    /// Whether the saved generation is certainly unusable, as opposed to failing to load
    /// for a reason that may pass, such as an I/O error. Only then may startup delete it.
    pub fn is_unusable_generation(&self) -> bool {
        matches!(
            self,
            Self::BadPointer(_) | Self::FormatVersion(_) | Self::MissingGeneration(_)
        )
    }
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

    /// Open a committed store read-only: it is never written again, and opening it
    /// read-write would add files to it and could start compactions while serving.
    fn open_read_only(path: &Path) -> Result<Self, DBError> {
        let db = DB::open_for_read_only(&Options::default(), path, false)?;

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
        let stores = (|| {
            Ok::<_, DBError>((
                DomainStore::create(&dir.join("blacklist"))?,
                DomainStore::create(&dir.join("whitelist"))?,
                DomainStore::create(&dir.join("rewrites"))?,
            ))
        })();

        match stores {
            Ok((blacklist, whitelist, rewrites)) => Ok(Self {
                blacklist,
                whitelist,
                rewrites,
                dir,
                discard: AtomicBool::new(true),
            }),
            Err(err) => {
                // No AdblockDB exists to delete the directory on drop, so do it here.
                let _ = fs::remove_dir_all(&dir);
                Err(err)
            }
        }
    }

    fn open(dir: PathBuf) -> Result<Self, DBError> {
        let db = Self {
            blacklist: DomainStore::open_read_only(&dir.join("blacklist"))?,
            whitelist: DomainStore::open_read_only(&dir.join("whitelist"))?,
            rewrites: DomainStore::open_read_only(&dir.join("rewrites"))?,
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
        match lock.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => return Err(DBError::Locked(root)),
            Err(fs::TryLockError::Error(err)) => return Err(err.into()),
        }

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
        if !is_generation_name(name) {
            return Err(DBError::BadPointer(pointer.clone()));
        }

        // Checked here because RocksDB creates a missing path even when asked only to open.
        let dir = self.root.join(name);
        if !dir.is_dir() {
            return Err(DBError::MissingGeneration(name.to_string()));
        }

        AdblockDB::open(dir).map(Some)
    }

    /// Remove every generation except `keep`, and a pointer left by an interrupted
    /// commit. Without a generation to keep, `current` goes too.
    ///
    /// Best effort: whatever cannot be removed is logged and left for the next start, so
    /// a stray entry never stops the server from starting.
    pub fn clean(&self, keep: Option<&AdblockDB>) {
        let keep = keep.map(|db| db.name());
        let mut files = vec![CURRENT_TMP_FILE];
        if keep.is_none() {
            files.push(CURRENT_FILE);
        }
        for file in files {
            if let Err(err) = remove_file_if_exists(&self.root.join(file)) {
                tracing::warn!("Removing {file} failed: {err}");
            }
        }

        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(err) => {
                tracing::warn!("Listing {} failed: {err}", self.root.display());
                return;
            }
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
            if !is_dir || !is_generation_name(&name) || keep.as_deref() == Some(name.as_str()) {
                continue;
            }

            tracing::info!("Removing stale db: {name}");
            if let Err(err) = fs::remove_dir_all(entry.path()) {
                tracing::warn!("Removing stale db: {name} failed: {err}");
            }
        }
    }

    /// Create an empty generation to compile into. It is deleted when dropped unless it
    /// is committed.
    pub fn new_generation(&self) -> Result<AdblockDB, DBError> {
        AdblockDB::create(self.root.join(generation_name()))
    }

    /// Flush `db` and make it the generation loaded on the next start.
    ///
    /// Synchronous: call this from a blocking context.
    ///
    /// On error the caller must not swap `db` in, nor delete the generation it would
    /// replace: the pointer may name either one after a power loss.
    pub fn commit(&self, db: &AdblockDB) -> Result<(), DBError> {
        db.flush()?;
        // RocksDB syncs its own files; this syncs the generation's store directories.
        sync_dir(&db.dir)?;

        let tmp = self.root.join(CURRENT_TMP_FILE);
        let mut file = File::create(&tmp)?;
        writeln!(file, "{FORMAT_VERSION} {}", db.name())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, self.root.join(CURRENT_FILE))?;

        // From here on the pointer names this generation, so it must not be deleted even
        // if the directory sync below fails.
        db.discard.store(false, Ordering::SeqCst);

        // Persist the rename. If this fails, a power loss could bring back the old
        // pointer, so the error stops the caller from deleting the old generation.
        sync_dir(&self.root)?;

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

        // leftovers: an interrupted compile and a half-written pointer
        let partial = compile(&dir, &["b.example"]);
        let partial_name = partial.name();
        std::mem::forget(partial);
        fs::write(root.join(CURRENT_TMP_FILE), "garbage").unwrap();
        fs::write(root.join("unrelated"), "kept").unwrap();
        assert!(generations(&root).contains(&partial_name));

        dir.clean(Some(&current));

        assert_eq!(generations(&root), vec![current.name()]);
        assert!(!root.join(CURRENT_TMP_FILE).exists());
        assert!(root.join(CURRENT_FILE).exists());
        assert!(root.join("unrelated").exists());
        drop(current);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clean_leaves_anything_that_is_not_a_generation() {
        let root = temp_root();
        let dir = DbDir::open(&root).unwrap();
        let current = compile(&dir, &["a.example"]);
        dir.commit(&current).unwrap();

        // look like generations but are files, or are not named exactly like one
        fs::write(root.join("gen-1-abcdefghij"), "a file").unwrap();
        fs::write(root.join("db-backup.sql"), "a file").unwrap();
        fs::create_dir_all(root.join("db-AbCdEfGhIj")).unwrap();
        fs::create_dir_all(root.join("gen-backup")).unwrap();
        fs::create_dir_all(root.join("gen-1-short")).unwrap();

        dir.clean(Some(&current));

        for kept in [
            "gen-1-abcdefghij",
            "db-backup.sql",
            "db-AbCdEfGhIj",
            "gen-backup",
            "gen-1-short",
        ] {
            assert!(root.join(kept).exists(), "{kept} was removed");
        }
        assert!(dir.load_current().unwrap().is_some());
        drop(current);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn generation_names() {
        assert!(is_generation_name(&generation_name()));
        assert!(is_generation_name("gen-1790000000-AbCdEfGhIj"));
        for name in [
            "gen-",
            "gen-1790000000",
            "gen-1790000000-",
            "gen-x790000000-AbCdEfGhIj",
            "gen-1790000000-AbCdEfGhI",
            "gen-1790000000-AbCdEfGh/j",
            "gen-1790000000-AbCdEfGhIjK",
            "db-AbCdEfGhIj",
        ] {
            assert!(!is_generation_name(name), "{name}");
        }
    }

    #[test]
    fn clean_without_generation_removes_pointer() {
        let root = temp_root();
        let dir = DbDir::open(&root).unwrap();
        let db = compile(&dir, &["a.example"]);
        dir.commit(&db).unwrap();
        std::mem::forget(db);

        dir.clean(None);

        assert!(generations(&root).is_empty());
        assert!(dir.load_current().unwrap().is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pointer_to_missing_or_foreign_generation_is_an_error() {
        let root = temp_root();
        let dir = DbDir::open(&root).unwrap();

        fs::write(root.join(CURRENT_FILE), "1 gen-1-AbCdEfGhIj\n").unwrap();
        assert!(matches!(
            dir.load_current(),
            Err(DBError::MissingGeneration(_))
        ));
        // RocksDB must not have been left to create it
        assert!(!root.join("gen-1-AbCdEfGhIj").exists());

        fs::write(root.join(CURRENT_FILE), "99 gen-1-AbCdEfGhIj\n").unwrap();
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
