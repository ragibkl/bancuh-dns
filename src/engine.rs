use std::sync::{Arc, Mutex};

use thiserror::Error;

use crate::{
    compiler::AdblockCompiler,
    config::{Config, FileOrUrl, LoadConfigError},
    db::AdblockDB,
};

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

    pub async fn run_update(&self) -> Result<(), EngineError> {
        let db = self.db.clone();
        let config_url = self.config_url.clone();

        // instantiate a new_db and load adblock definition into it
        let new_db = AdblockDB::create()?;
        load_definition(&new_db, &config_url).await?;

        // swap the new_db in-place of the existing old_db and destroy old_db
        *db.lock().expect("Could not lock db") = new_db;

        Ok(())
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
