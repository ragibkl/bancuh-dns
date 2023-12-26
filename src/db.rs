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

fn domain_exists(db: &DB, key: &str) -> bool {
    let parts: Vec<&str> = key.split('.').filter(|s| !s.is_empty()).collect();

    let mut tests: Vec<String> = Vec::new();
    let mut acc: Vec<String> = Vec::new();
    for part in parts.iter().skip(1).rev() {
        acc.splice(0..0, vec![part.to_string()]);
        let test = format!("*.{}.", acc.join("."));
        tests.push(test)
    }
    tests.push(key.to_string());

    for test in tests.iter() {
        if db.get(test).unwrap().is_some() {
            return true;
        }
    }

    false
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
    bl: DomainStore,
    wl: DomainStore,
    rw: DomainStore,
}

impl AdblockDB {
    pub fn new() -> Self {
        let bl = DomainStore::new().unwrap();
        let wl = DomainStore::new().unwrap();
        let rw = DomainStore::new().unwrap();

        Self { bl, wl, rw }
    }

    pub fn insert_whitelist(&self, domain: &str) {
        self.wl.db.put(format!("{domain}."), "true").unwrap();
    }

    pub fn insert_blacklist(&self, domain: &str) {
        self.bl.db.put(format!("{domain}."), "true").unwrap();
    }

    pub fn insert_rewrite(&self, domain: &str, target: &str) {
        self.rw.db.put(format!("{domain}."), target).unwrap();
    }

    pub fn bl_exist(&self, domain: &str) -> bool {
        domain_exists(&self.bl.db, domain)
    }

    pub fn wl_exist(&self, domain: &str) -> bool {
        domain_exists(&self.wl.db, domain)
    }

    pub fn get_rewrite(&self, domain: &str) -> Option<String> {
        let bytes = self.rw.db.get(domain).unwrap();
        bytes.map(|b| String::from_utf8(b).unwrap())
    }

    pub fn destroy(self) {
        self.bl.destroy();
        self.wl.destroy();
        self.rw.destroy();
    }
}

impl Default for AdblockDB {
    fn default() -> Self {
        Self::new()
    }
}
