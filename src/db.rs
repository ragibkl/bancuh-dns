use std::path::PathBuf;

use rand::{distributions::Alphanumeric, thread_rng, Rng};
use rocksdb::{Options, DB};

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
pub struct AdblockDB {
    bl: DB,
    wl: DB,
    rw: DB,
}

impl AdblockDB {
    pub fn new() -> Self {
        let dir: PathBuf = "./bancuh_db".parse().unwrap();
        let rand_prefix = rand_string();
        let wl_path = dir.join(format!("{rand_prefix}-whitelist"));
        let bl_path = dir.join(format!("{rand_prefix}-blacklist"));
        let rw_path = dir.join(format!("{rand_prefix}-rewrites"));

        let wl = DB::open_default(wl_path).unwrap();
        let bl = DB::open_default(bl_path).unwrap();
        let rw = DB::open_default(rw_path).unwrap();

        Self { bl, wl, rw }
    }

    pub fn insert_whitelist(&self, domain: &str) {
        self.wl.put(format!("{domain}."), "true").unwrap();
    }

    pub fn insert_blacklist(&self, domain: &str) {
        self.bl.put(format!("{domain}."), "true").unwrap();
    }

    pub fn insert_rewrite(&self, domain: &str, target: &str) {
        self.rw.put(format!("{domain}."), target).unwrap();
    }

    pub fn bl_exist(&self, domain: &str) -> bool {
        domain_exists(&self.bl, domain)
    }

    pub fn wl_exist(&self, domain: &str) -> bool {
        domain_exists(&self.wl, domain)
    }

    pub fn get_rewrite(&self, domain: &str) -> Option<String> {
        let bytes = self.rw.get(domain).unwrap();
        bytes.map(|b| String::from_utf8(b).unwrap())
    }
}

impl Default for AdblockDB {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AdblockDB {
    fn drop(&mut self) {
        let bl = self.bl.path().to_path_buf();
        let wl = self.wl.path().to_path_buf();
        let rw = self.rw.path().to_path_buf();

        tokio::task::spawn_blocking(move || {
            std::thread::sleep(std::time::Duration::from_micros(100));

            let opts = Options::default();
            let _ = DB::destroy(&opts, bl);
            let _ = DB::destroy(&opts, wl);
            let _ = DB::destroy(&opts, rw);
        });
    }
}
