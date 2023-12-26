use std::path::PathBuf;

use rand::{distributions::Alphanumeric, thread_rng, Rng};
use rocksdb::{DBWithThreadMode, Options, SingleThreaded, ThreadMode, DB};

fn rand_string() -> String {
    thread_rng()
        .sample_iter(&Alphanumeric)
        .take(30)
        .map(char::from)
        .collect()
}

fn domain_exists<T: ThreadMode>(db: &DBWithThreadMode<T>, key: &str) -> bool {
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
    db: DBWithThreadMode<SingleThreaded>,
    path: PathBuf,
}

impl DomainStore {
    pub fn new() -> Result<Self, rocksdb::Error> {
        let dir: PathBuf = "./bancuh_db".parse().unwrap();
        let rand_prefix = rand_string();
        let path = dir.join(format!("{rand_prefix}-db"));
        let db = DBWithThreadMode::<SingleThreaded>::open_default(&path)?;

        Ok(Self { db, path })
    }
}

impl Drop for DomainStore {
    fn drop(&mut self) {
        let path = self.path.to_path_buf();
        self.db.cancel_all_background_work(true);

        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_micros(100)).await;

            let opts = Options::default();

            let path_str = path.to_path_buf().to_string_lossy().to_string();
            tracing::info!("Destroying db: {path_str}",);
            let del_ok = DB::destroy(&opts, path).is_ok();
            tracing::info!("Destroying db: {path_str}. Ok: {del_ok}");

            {}
        });
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
}

impl Default for AdblockDB {
    fn default() -> Self {
        Self::new()
    }
}
