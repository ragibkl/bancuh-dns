use std::{path::PathBuf, string::FromUtf8Error};

use rand::{Rng, distr::Alphanumeric};
use rocksdb::{DBWithThreadMode, MultiThreaded, Options, WriteBatch};
use thiserror::Error;

pub type DB = DBWithThreadMode<MultiThreaded>;

/// Entries per RocksDB write batch. Batching amortises the per-write overhead across the
/// millions of domains a compile writes; the cap keeps any single batch small in memory.
const WRITE_BATCH_SIZE: usize = 10_000;

fn rand_string() -> String {
    rand::rng()
        .sample_iter(Alphanumeric)
        .take(10)
        .map(char::from)
        .collect()
}

fn rand_name() -> String {
    format!("db-{}", rand_string())
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
}

#[derive(Debug)]
pub struct DomainStore {
    db: Option<DB>,
}

impl DomainStore {
    pub fn create() -> Result<Self, DBError> {
        let dir: PathBuf = "./bancuh_db".parse().expect("Unexpected path parse error");
        let path = dir.join(rand_name());

        let db = DB::open_default(path)?;
        let db = Some(db);

        Ok(Self { db })
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
            batch.put(normalize_name(domain.as_ref()), normalize_name(alias.as_ref()));
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

impl Drop for DomainStore {
    fn drop(&mut self) {
        if let Some(db) = std::mem::take(&mut self.db) {
            let path = db.path().to_path_buf();
            let path_str = path.to_string_lossy().to_string();
            let opts = Options::default();

            tracing::info!("Destroying db: {path_str}");
            db.cancel_all_background_work(true);
            drop(db);
            self.db = None;
            let res = DB::destroy(&opts, path);
            tracing::info!("Destroying db: {path_str}. DONE: {res:?}");
        }
    }
}

#[derive(Debug)]
pub struct AdblockDB {
    pub blacklist: DomainStore,
    pub whitelist: DomainStore,
    pub rewrites: DomainStore,
}

impl AdblockDB {
    pub fn create() -> Result<Self, DBError> {
        let blacklist = DomainStore::create()?;
        let whitelist = DomainStore::create()?;
        let rewrites = DomainStore::create()?;

        Ok(Self {
            blacklist,
            whitelist,
            rewrites,
        })
    }
}
