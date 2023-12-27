use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use thiserror::Error;
use tokio::task::JoinHandle;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    compiler::AdblockCompiler,
    config::{Config, FileOrUrl, LoadConfigError},
    db::AdblockDB,
};

static UPDATE_INTERVAL: Duration = Duration::from_secs(86400); // 1 day

async fn load_definition(db: &AdblockDB, config_url: &FileOrUrl) -> Result<(), LoadConfigError> {
    tracing::info!("Loading adblock config. config_url: {config_url}");
    let config = Config::load(config_url).await?;
    let compiler = AdblockCompiler::from_config(&config);
    tracing::info!("Loading adblock config. config_url: {config_url}. DONE");

    tracing::info!("Compiling adblock");
    compiler.compile(db).await;
    tracing::info!("Compiling adblock DONE");

    Ok(())
}

pub struct UpdateHandle {
    token: CancellationToken,
    tracker: TaskTracker,
}

impl UpdateHandle {
    pub async fn shutdown_gracefully(&self) {
        self.token.cancel();
        self.tracker.wait().await;
    }

    pub async fn task_terminated(&self) {
        self.tracker.wait().await;
    }
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error(transparent)]
    DB(#[from] crate::db::DBError),

    #[error(transparent)]
    LoadConfig(#[from] crate::config::LoadConfigError),
}

#[derive(Debug)]
pub struct AdblockEngine {
    db: Arc<Mutex<AdblockDB>>,
    config_url: FileOrUrl,
}

impl AdblockEngine {
    pub fn new(config_url: FileOrUrl) -> Result<Self, EngineError> {
        let db = Arc::new(Mutex::new(AdblockDB::create()?));

        Ok(Self { db, config_url })
    }

    pub fn start_update(&self) -> UpdateHandle {
        let db = self.db.clone();
        let config_url = self.config_url.clone();

        let tracker = TaskTracker::new();
        let token = CancellationToken::new();

        let cloned_token = token.clone();
        tracker.spawn(async move {
            loop {
                // instantiate a new_db and load adblock definition into it
                let new_db = AdblockDB::create()?;
                load_definition(&new_db, &config_url).await?;

                // swap the new_db in-place of the existing old_db and destroy old_db
                let old_db = std::mem::replace(&mut *db.lock().expect("Could not lock db"), new_db);
                old_db.destroy();

                tracing::info!("Sleeping...");

                tokio::select! {
                    _ = tokio::time::sleep(UPDATE_INTERVAL) => {}
                    _ = cloned_token.cancelled() => {
                        tracing::info!("Shutting down updater task...");
                        let owned_db = Arc::try_unwrap(db).expect("Unexpected references to owned_db");
                        let owned_db = owned_db.into_inner().expect("Could not lock db");
                        owned_db.destroy();

                        return Ok::<(), EngineError>(());
                    }
                }
            }
        });
        tracker.close();

        UpdateHandle { token, tracker }
    }

    pub async fn get_redirect(&self, name: &str) -> Result<Option<String>, EngineError> {
        let db_guard = self.db.lock().expect("Could not lock db");
        let alias = db_guard.rewrites.get(name)?;

        if let Some(alias) = alias.as_deref() {
            tracing::info!("rewrite: {name} to: {alias}");
        }

        Ok(alias)
    }

    pub async fn is_blocked(&self, name: &str) -> Result<bool, EngineError> {
        let db_guard = self.db.lock().expect("Could not lock db");

        if db_guard.whitelist.contains(name)? {
            tracing::info!("whitelist: {name}");
            return Ok(false);
        }

        if db_guard.blacklist.contains(name)? {
            tracing::info!("blacklist: {name}");
            return Ok(true);
        }

        Ok(false)
    }
}
