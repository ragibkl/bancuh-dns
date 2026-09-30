use std::{path::PathBuf, sync::Arc};

use arc_swap::ArcSwapOption;
use thiserror::Error;
use tokio::sync::watch;

use crate::{
    compiler::{AdblockCompiler, CompileError},
    config::{Config, FileOrUrl},
    db::{AdblockDB, DbDir},
};

async fn load_definition(db: Arc<AdblockDB>, config_url: &FileOrUrl) -> Result<(), EngineError> {
    tracing::info!("Loading adblock config. config_url: {config_url}");
    let config = Config::load(config_url).await?;
    let compiler = AdblockCompiler::from_config(&config);
    tracing::info!("Loading adblock config. config_url: {config_url}. DONE");

    tracing::info!("Compiling adblock");
    compiler.compile(db).await?;
    tracing::info!("Compiling adblock DONE");

    Ok(())
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error(transparent)]
    DB(#[from] crate::db::DBError),

    #[error(transparent)]
    LoadConfig(#[from] crate::config::LoadConfigError),

    #[error(transparent)]
    Compile(#[from] CompileError),

    #[error("db task failed to complete: {0}")]
    Join(#[from] tokio::task::JoinError),

    #[error("no blocklist loaded yet")]
    NotReady,
}

#[derive(Debug)]
pub struct AdblockEngine {
    /// `None` until a blocklist is loaded: either the generation saved by the previous
    /// run, or the first compile. The server does not answer queries before then.
    db: ArcSwapOption<AdblockDB>,
    dir: Arc<DbDir>,
    config_url: FileOrUrl,
    ready: watch::Sender<bool>,
}

impl AdblockEngine {
    /// Open the db dir, load the generation saved by the previous run if it is usable,
    /// and remove everything else in the dir.
    pub fn new(config_url: FileOrUrl, db_dir: PathBuf) -> Result<Self, EngineError> {
        tracing::info!("Opening db dir: {}", db_dir.display());
        let dir = DbDir::open(db_dir)?;

        let saved = match dir.load_current() {
            Ok(Some(db)) => {
                let age = db
                    .compiled_at()
                    .map(|t| format!("{}s", (chrono::Utc::now() - t).num_seconds()))
                    .unwrap_or_else(|| "unknown".to_string());
                tracing::info!("Loaded saved blocklist {} (age {age})", db.name());
                Some(db)
            }
            Ok(None) => {
                tracing::info!("No saved blocklist; the first compile must finish before serving");
                None
            }
            Err(err) => {
                tracing::warn!(
                    "Saved blocklist is unusable: {err}; the first compile must finish before serving"
                );
                None
            }
        };
        dir.clean(saved.as_ref())?;

        let (ready, _) = watch::channel(saved.is_some());

        Ok(Self {
            db: ArcSwapOption::from(saved.map(Arc::new)),
            dir: Arc::new(dir),
            config_url,
            ready,
        })
    }

    /// Whether a blocklist is loaded and queries can be answered.
    pub fn is_ready(&self) -> bool {
        *self.ready.borrow()
    }

    /// Wait until a blocklist is loaded.
    pub async fn wait_ready(&self) {
        let mut rx = self.ready.subscribe();
        // the sender lives as long as self, so this cannot fail
        let _ = rx.wait_for(|ready| *ready).await;
    }

    pub async fn run_update(&self) -> Result<(), EngineError> {
        let config_url = self.config_url.clone();

        // instantiate a new_db and load adblock definition into it. A failed write now
        // propagates, and the new generation is deleted when dropped rather than swapped in.
        let dir = self.dir.clone();
        let new_db = Arc::new(tokio::task::spawn_blocking(move || dir.new_generation()).await??);
        load_definition(new_db.clone(), &config_url).await?;

        // make it the generation the next start loads, before serving from it
        let dir = self.dir.clone();
        let committed = new_db.clone();
        tokio::task::spawn_blocking(move || dir.commit(&committed)).await??;

        // atomically swap the new_db in place; the old generation is deleted from disk
        // once the last in-flight query drops it
        if let Some(old_db) = self.db.swap(Some(new_db)) {
            old_db.set_discard();
        }
        self.ready.send_replace(true);

        Ok(())
    }

    pub async fn get_redirect(&self, name: &str) -> Result<Option<String>, EngineError> {
        let db_guard = self.db.load();
        let db = db_guard.as_ref().ok_or(EngineError::NotReady)?;
        let alias = db.rewrites.get(name)?;

        if let Some(alias) = alias.as_deref() {
            tracing::debug!("rewrite: {name} to: {alias}");
        }

        Ok(alias)
    }

    pub async fn is_blocked(&self, name: &str) -> Result<bool, EngineError> {
        let db_guard = self.db.load();
        let db = db_guard.as_ref().ok_or(EngineError::NotReady)?;

        if db.whitelist.contains(name)? {
            tracing::debug!("whitelist: {name}");
            return Ok(false);
        }

        if db.blacklist.contains(name)? {
            tracing::debug!("blacklist: {name}");
            return Ok(true);
        }

        Ok(false)
    }
}
