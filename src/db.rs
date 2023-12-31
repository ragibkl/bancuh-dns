use std::{path::PathBuf, string::FromUtf8Error};

use rand::{distributions::Alphanumeric, thread_rng, Rng};
use rocksdb::{DBWithThreadMode, MultiThreaded, Options};
use thiserror::Error;

pub type DB = DBWithThreadMode<MultiThreaded>;

fn rand_string() -> String {
    thread_rng()
        .sample_iter(&Alphanumeric)
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

    pub fn put(&self, domain: &str) -> Result<(), DBError> {
        if let Some(db) = &self.db {
            let domain = normalize_name(domain);
            db.put(domain, "true")?;
        }

        Ok(())
    }

    pub fn put_alias(&self, domain: &str, alias: &str) -> Result<(), DBError> {
        if let Some(db) = &self.db {
            let domain = normalize_name(domain);
            let alias = normalize_name(alias);
            db.put(domain, alias)?;
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
