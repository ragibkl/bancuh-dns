use std::{str::FromStr, sync::Arc, time::Duration};

use tokio::sync::Mutex;

use crate::{
    compiler::AdblockCompiler,
    config::{ConfigUrl, LoadConfig},
    db::AdblockDB,
};

static UPDATE_INTERVAL: Duration = Duration::from_secs(86400); // 1 day

#[derive(Debug)]
pub struct AdblockEngine {
    db: Arc<Mutex<AdblockDB>>,
    config_url: ConfigUrl,
}

impl AdblockEngine {
    pub fn new() -> Self {
        let db = Arc::new(Mutex::new(AdblockDB::new()));
        let config_url = ConfigUrl::from_str("./data/configuration.yaml").unwrap();

        Self { db, config_url }
    }

    pub async fn start_update(&mut self) {
        let db = self.db.clone();
        let config_url = self.config_url.clone();

        tracing::info!("Loading adblock config on first run");
        let config = LoadConfig::from(&config_url).load().await.unwrap();
        let compiler = AdblockCompiler::init(&config).unwrap();
        tracing::info!("Loading adblock config on first run DONE");

        tracing::info!("Compiling adblock on first run");
        compiler.compile(db.clone()).await;
        tracing::info!("Compiling adblock on first run DONE");

        tokio::spawn(async move {
            loop {
                tracing::info!("Sleeping for ONE_DAY...");
                tokio::time::sleep(UPDATE_INTERVAL).await;

                tracing::info!("Loading adblock config");
                let config = LoadConfig::from(&config_url).load().await.unwrap();
                let compiler = AdblockCompiler::init(&config).unwrap();
                tracing::info!("Loading adblock config DONE");

                tracing::info!("Compiling adblock");
                let new_db = Arc::new(Mutex::new(AdblockDB::new()));
                compiler.compile(new_db.clone()).await;
                let new_db = Arc::into_inner(new_db).unwrap().into_inner();
                tracing::info!("Compiling adblock DONE");

                let mut db_guard = db.lock().await;
                let old_db = std::mem::replace(&mut *db_guard, new_db);
                old_db.destroy();
            }
        });
    }

    pub async fn get_redirect(&self, name: &str) -> Option<String> {
        let db_guard = self.db.lock().await;
        let alias = db_guard.get_rewrite(name);

        if let Some(alias) = alias.as_deref() {
            tracing::info!("rewrite: {name} to: {alias}");
        }

        alias
    }

    pub async fn is_blocked(&self, name: &str) -> bool {
        let db_guard = self.db.lock().await;

        if db_guard.wl_exist(name) {
            tracing::info!("whitelist: {name}");
            return false;
        }

        if db_guard.bl_exist(name) {
            tracing::info!("blacklist: {name}");
            return true;
        }

        false
    }
}
