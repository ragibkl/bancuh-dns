use std::str::FromStr;

use crate::{
    compiler::AdblockCompiler,
    config::{ConfigUrl, LoadConfig},
};

use crate::db::AdblockDB;

#[derive(Debug)]
pub struct AdblockEngine {
    db: AdblockDB,
    config_url: ConfigUrl,
}

impl AdblockEngine {
    pub fn new() -> Self {
        let db = AdblockDB::new();
        let config_url = ConfigUrl::from_str("./data/configuration.yaml").unwrap();

        Self { db, config_url }
    }

    pub async fn fetch(&mut self) {
        let config = LoadConfig::from(&self.config_url).load().await.unwrap();
        let compiler = AdblockCompiler::init(&config).unwrap();

        compiler.compile(&self.db).await;
    }

    pub async fn get_redirect(&self, name: &str) -> Option<String> {
        self.db.get_rewrite(name)
    }

    pub async fn is_blocked(&self, name: &str) -> bool {
        if self.db.wl_exist(name) {
            tracing::info!("whitelist: {name}");
            return false;
        }

        if self.db.bl_exist(name) {
            tracing::info!("blacklist: {name}");
            return true;
        }

        false
    }
}
