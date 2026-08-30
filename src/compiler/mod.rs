mod blacklist;
mod parser;
mod rewrites;
mod whitelist;

use std::sync::Arc;

use thiserror::Error;

use crate::{config::Config, db::AdblockDB};

use self::{
    blacklist::BlacklistCompiler, rewrites::RewritesCompiler, whitelist::WhitelistCompiler,
};

#[derive(Debug, Error)]
pub enum CompileError {
    #[error(transparent)]
    DB(#[from] crate::db::DBError),

    #[error("compile task failed to complete: {0}")]
    Join(#[from] tokio::task::JoinError),
}

#[derive(Debug)]
pub struct AdblockCompiler {
    blacklists: Vec<BlacklistCompiler>,
    whitelists: Vec<WhitelistCompiler>,
    rewrites: Vec<RewritesCompiler>,
}

impl AdblockCompiler {
    pub fn from_config(config: &Config) -> Self {
        let blacklists = config.blacklist.iter().map(|bl| bl.into()).collect();
        let whitelists = config.whitelist.iter().map(|wl| wl.into()).collect();
        let rewrites = config.overrides.iter().map(|rw| rw.into()).collect();

        Self {
            blacklists,
            whitelists,
            rewrites,
        }
    }

    /// Compile every configured source into `db`.
    ///
    /// The writes are synchronous RocksDB calls over millions of domains. Running them
    /// inline holds the async worker thread for the length of each source, which starves
    /// the DNS handler on a single-core host, so each batch is handed to the blocking
    /// pool instead.
    pub async fn compile(&self, db: Arc<AdblockDB>) -> Result<(), CompileError> {
        for wl in &self.whitelists {
            let domains = wl.load_whitelist().await;
            let db = db.clone();
            tokio::task::spawn_blocking(move || {
                db.whitelist.put_all(domains.iter().map(|d| d.0.as_str()))
            })
            .await??;
        }

        for bl in &self.blacklists {
            let domains = bl.load_blacklist().await;
            let db = db.clone();
            tokio::task::spawn_blocking(move || {
                db.blacklist.put_all(domains.iter().map(|d| d.0.as_str()))
            })
            .await??;
        }

        for rw in &self.rewrites {
            let cnames = rw.load_rewrites().await;
            let db = db.clone();
            tokio::task::spawn_blocking(move || {
                db.rewrites
                    .put_aliases_all(cnames.iter().map(|c| (c.domain.0.as_str(), c.alias.0.as_str())))
            })
            .await??;
        }

        Ok(())
    }
}
