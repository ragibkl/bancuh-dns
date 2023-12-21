mod blacklist;
mod parser;
mod rewrites;
mod whitelist;

use std::sync::Arc;

use tokio::sync::Mutex;

use crate::{config::Config, db::AdblockDB};

use self::{
    blacklist::BlacklistCompiler, rewrites::RewritesCompiler, whitelist::WhitelistCompiler,
};

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

    pub async fn compile(&self, db: Arc<Mutex<AdblockDB>>) {
        for wl in &self.whitelists {
            let domains = wl.load_whitelist().await;
            {
                let db_guard = db.lock().await;
                for d in domains {
                    db_guard.insert_whitelist(&d.0);
                }
            }
        }

        for bl in &self.blacklists {
            let domains = bl.load_blacklist().await;
            {
                let db_guard = db.lock().await;
                for d in domains {
                    db_guard.insert_blacklist(&d.0);
                }
            }
        }

        for rw in &self.rewrites {
            let cnames = rw.load_rewrites().await;
            {
                let db_guard = db.lock().await;
                for c in cnames {
                    db_guard.insert_rewrite(&c.domain.0, &c.alias.0);
                }
            }
        }
    }
}
