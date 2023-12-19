use std::path::PathBuf;

use rand::{distributions::Alphanumeric, thread_rng, Rng};
use rocksdb::DB;

fn rand_string() -> String {
    thread_rng()
        .sample_iter(&Alphanumeric)
        .take(30)
        .map(char::from)
        .collect()
}

#[derive(Debug)]
pub struct AdblockDB {
    bl: DB,
    wl: DB,
}

impl AdblockDB {
    pub fn new() -> Self {
        let dir: PathBuf = "./bancuh_db".parse().unwrap();
        let sub_dir = dir.join(rand_string());
        let wl_path = sub_dir.join("whitelist");
        let bl_path = sub_dir.join("blacklist");

        let wl = DB::open_default(wl_path).unwrap();
        let bl = DB::open_default(bl_path).unwrap();

        Self { bl, wl }
    }

    pub fn insert_whitelist(&self, domain: &str) {
        self.wl.put(format!("{domain}."), "true").unwrap();
    }

    pub fn insert_blacklist(&self, domain: &str) {
        self.bl.put(format!("{domain}."), "true").unwrap();
    }

    pub fn bl_exist(&self, domain: &str) -> bool {
        self.bl.get(domain).unwrap().is_some()
    }

    pub fn wl_exist(&self, domain: &str) -> bool {
        self.wl.get(domain).unwrap().is_some()
    }
}
