use std::{sync::Arc, time::Duration};

use tokio::sync::Mutex;

use crate::{
    compiler::AdblockCompiler,
    config::{ConfigUrl, LoadConfig},
    db::AdblockDB,
};

static UPDATE_INTERVAL: Duration = Duration::from_secs(86400); // 1 day

async fn update_definition(db: Arc<Mutex<AdblockDB>>, config_url: &ConfigUrl) {
    tracing::info!("Loading adblock config. config_url: {config_url}");
    let config = LoadConfig::from(config_url).load().await.unwrap();
    let compiler = AdblockCompiler::init(&config).unwrap();
    tracing::info!("Loading adblock config. config_url: {config_url}. DONE");

    tracing::info!("Compiling adblock");
    compiler.compile(db).await;
    tracing::info!("Compiling adblock DONE");
}

#[derive(Debug)]
pub struct AdblockEngine {
    db: Arc<Mutex<AdblockDB>>,
    config_url: ConfigUrl,
}

impl AdblockEngine {
    pub fn new(config_url: ConfigUrl) -> Self {
        let db = Arc::new(Mutex::new(AdblockDB::new()));

        Self { db, config_url }
    }

    pub fn start_update(&self) {
        let db = self.db.clone();
        let config_url = self.config_url.clone();

        tokio::spawn(async move {
            update_definition(db.clone(), &config_url).await;

            loop {
                tracing::info!("Sleeping...");
                tokio::time::sleep(UPDATE_INTERVAL).await;

                let new_db = Arc::new(Mutex::new(AdblockDB::new()));
                update_definition(new_db.clone(), &config_url).await;
                let new_db = Arc::into_inner(new_db).unwrap().into_inner();

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
