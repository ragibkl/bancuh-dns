use std::path::PathBuf;

use rand::{distributions::Alphanumeric, thread_rng, Rng};
use rocksdb::{DBWithThreadMode, MultiThreaded, Options};

pub type DB = DBWithThreadMode<MultiThreaded>;

fn rand_string() -> String {
    thread_rng()
        .sample_iter(&Alphanumeric)
        .take(30)
        .map(char::from)
        .collect()
}

fn normalize_name(name: &str) -> String {
    if name.ends_with('.') {
        name.to_string()
    } else {
        format!("{name}.")
    }
}

#[derive(Debug)]
pub struct DomainStore {
    db: DB,
}

impl DomainStore {
    pub fn new() -> Result<Self, rocksdb::Error> {
        let dir: PathBuf = "./bancuh_db".parse().unwrap();
        let rand_prefix = rand_string();
        let path = dir.join(format!("{rand_prefix}-db"));
        let db = DB::open_default(path)?;

        Ok(Self { db })
    }

    pub fn put(&self, domain: &str) {
        let domain = normalize_name(domain);
        self.db.put(domain, "true").unwrap();
    }

    pub fn put_alias(&self, domain: &str, alias: &str) {
        let domain = normalize_name(domain);
        let alias = normalize_name(alias);
        self.db.put(domain, alias).unwrap();
    }

    pub fn get(&self, domain: &str) -> Option<String> {
        let parts: Vec<&str> = domain.split('.').filter(|s| !s.is_empty()).collect();

        let mut keys: Vec<String> = vec![domain.to_string()];
        for i in 1..parts.len() {
            let star_key = format!("*.{}.", parts[i..parts.len()].join("."));
            keys.push(star_key);
        }

        for key in keys.iter() {
            if let Some(s) = self.db.get(key).unwrap() {
                return Some(String::from_utf8(s).unwrap());
            }
        }

        None
    }

    pub fn contains(&self, domain: &str) -> bool {
        self.get(domain).is_some()
    }

    pub fn destroy(self) {
        let path = self.db.path().to_path_buf();
        let path_str = path.to_string_lossy().to_string();
        let opts = Options::default();

        tracing::info!("Destroying db: {path_str}");
        self.db.cancel_all_background_work(true);
        drop(self);
        let res = DB::destroy(&opts, path);
        tracing::info!("Destroying db: {path_str}. DONE: {res:?}");
    }
}

#[derive(Debug)]
pub struct AdblockDB {
    pub blacklist: DomainStore,
    pub whitelist: DomainStore,
    pub rewrites: DomainStore,
}

impl AdblockDB {
    pub fn new() -> Self {
        let blacklist = DomainStore::new().unwrap();
        let whitelist = DomainStore::new().unwrap();
        let rewrites = DomainStore::new().unwrap();

        Self {
            blacklist,
            whitelist,
            rewrites,
        }
    }

    pub fn destroy(self) {
        self.blacklist.destroy();
        self.whitelist.destroy();
        self.rewrites.destroy();
    }
}

impl Default for AdblockDB {
    fn default() -> Self {
        Self::new()
    }
}
