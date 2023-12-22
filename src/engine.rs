use std::{sync::Arc, time::Duration};

use tokio::sync::Mutex;

use crate::{
    compiler::AdblockCompiler,
    config::{Config, FileOrUrl},
    db::AdblockDB,
};

static UPDATE_INTERVAL: Duration = Duration::from_secs(86400); // 1 day

async fn load_definition(db: &AdblockDB, config_url: &FileOrUrl) {
    tracing::info!("Loading adblock config. config_url: {config_url}");
    let config = Config::load(config_url).await.unwrap();
    let compiler = AdblockCompiler::from_config(&config);
    tracing::info!("Loading adblock config. config_url: {config_url}. DONE");

    tracing::info!("Compiling adblock");
    compiler.compile(db).await;
    tracing::info!("Compiling adblock DONE");
}

#[derive(Debug)]
pub struct AdblockEngine {
    db: Arc<Mutex<AdblockDB>>,
    config_url: FileOrUrl,
}

impl AdblockEngine {
    pub fn new(config_url: FileOrUrl) -> Self {
        let db = Default::default();
        Self { db, config_url }
    }

    pub fn start_update(&self) {
        let db = self.db.clone();
        let config_url = self.config_url.clone();

        tokio::spawn(async move {
            loop {
                // instantiate a new_db and load adblock definition into it
                let new_db = AdblockDB::default();
                load_definition(&new_db, &config_url).await;

                // swap the new_db in-place of the existing old_db
                let old_db = {
                    let mut db_guard = db.lock().await;
                    std::mem::replace(&mut *db_guard, new_db)
                };
                old_db.destroy();

                tracing::info!("Sleeping...");
                tokio::time::sleep(UPDATE_INTERVAL).await;
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
