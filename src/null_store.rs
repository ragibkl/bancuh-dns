use std::str::FromStr;

use crate::{config::ConfigUrl, db::AdblockDB};

#[derive(Debug)]
pub struct NullStore {
    db: AdblockDB,
    config_url: ConfigUrl,
}

impl NullStore {
    pub fn new() -> Self {
        let db = AdblockDB::new();
        let config_url = ConfigUrl::from_str("./data/configuration.yaml").unwrap();

        Self { db, config_url }
    }

    pub async fn fetch(&mut self) {
        self.db.insert_blacklist("zedo.com");
        self.db.insert_blacklist("doubleclick.net");
    }

    pub async fn is_blocked(&self, name: &str) -> bool {
        self.db.bl_exist(name)
    }
}
