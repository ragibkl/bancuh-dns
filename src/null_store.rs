use std::{path::PathBuf, str::FromStr};

use rocksdb::DB;

#[derive(Clone, Debug)]
pub struct NullStore {
    dir: PathBuf,
}

impl NullStore {
    pub fn new() -> Self {
        let dir = PathBuf::from_str("./.").unwrap();
        Self { dir }
    }

    fn db(&self) -> DB {
        let path = self.dir.join("bancuh_db");
        DB::open_default(path).unwrap()
    }

    pub fn fetch(&mut self) {
        let db = self.db();
        db.put("zedo.com.", "true").unwrap();
        db.put("doubleclick.net.", "true").unwrap();
    }

    pub async fn is_blocked(&self, name: &str) -> bool {
        let db = self.db();
        let res = db.get(name).unwrap_or_default();

        res.is_some()
    }
}
