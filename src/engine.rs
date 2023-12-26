use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio_util::sync::CancellationToken;

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
    cancellation_token: CancellationToken,
}

impl AdblockEngine {
    pub fn new(config_url: FileOrUrl) -> Self {
        let db = Default::default();
        let cancellation_token = CancellationToken::new();

        Self {
            db,
            config_url,
            cancellation_token,
        }
    }

    pub fn start_update(&self) {
        let db = self.db.clone();
        let cancellation_token = self.cancellation_token.clone();
        let config_url = self.config_url.clone();

        tokio::spawn(async move {
            loop {
                // instantiate a new_db and load adblock definition into it
                let new_db = AdblockDB::default();
                load_definition(&new_db, &config_url).await;

                // swap the new_db in-place of the existing old_db
                let old_db = std::mem::replace(&mut *db.lock().unwrap(), new_db);
                old_db.destroy();

                tracing::info!("Sleeping...");

                tokio::select! {
                    _ = tokio::time::sleep(UPDATE_INTERVAL) => {}
                    _ = cancellation_token.cancelled() => {
                        tracing::info!("Shutting down updater task...");
                        let owned_db = Arc::try_unwrap(db).unwrap().into_inner().unwrap();
                        owned_db.destroy();
                        return;
                    }
                }
            }
        });
    }

    pub async fn get_redirect(&self, name: &str) -> Option<String> {
        let db_guard = self.db.lock().unwrap();
        let alias = db_guard.get_rewrite(name);

        if let Some(alias) = alias.as_deref() {
            tracing::info!("rewrite: {name} to: {alias}");
        }

        alias
    }

    pub async fn is_blocked(&self, name: &str) -> bool {
        let db_guard = self.db.lock().unwrap();

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

impl Drop for AdblockEngine {
    fn drop(&mut self) {
        self.cancellation_token.cancel();
    }
}
