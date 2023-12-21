use std::sync::Arc;

use thiserror::Error;
use tokio::sync::Mutex;

use crate::{config::Config, db::AdblockDB};

use super::{
    blacklist::{BlacklistCompiler, ParseBlacklist},
    fetch_source::{FetchSource, FetchSourceInitError},
    rewrites::{ParseRewrite, RewritesCompiler},
    whitelist::{ParseWhitelist, WhitelistCompiler},
};

#[derive(Debug)]
pub struct AdblockCompiler {
    blacklists: Vec<BlacklistCompiler>,
    whitelists: Vec<WhitelistCompiler>,
    rewrites: Vec<RewritesCompiler>,
}

#[derive(Debug, Error)]
pub enum AdblockCompilerConfigError {
    #[error("{0}")]
    FetchSourceError(#[from] FetchSourceInitError),
}

impl AdblockCompiler {
    pub fn init(config: &Config) -> Self {
        let blacklists = config
            .blacklist
            .iter()
            .map(|bl| {
                let source = FetchSource(bl.file_or_url.to_fetch());
                let parser = ParseBlacklist::from(&bl.format);

                BlacklistCompiler { source, parser }
            })
            .collect();

        let whitelists = config
            .whitelist
            .iter()
            .map(|wl| {
                let source = FetchSource(wl.file_or_url.to_fetch());
                let parser = ParseWhitelist::from(&wl.format);

                WhitelistCompiler { source, parser }
            })
            .collect();

        let rewrites = config
            .overrides
            .iter()
            .map(|rw| {
                let source = FetchSource(rw.file_or_url.to_fetch());
                let parser = ParseRewrite::from(&rw.format);

                RewritesCompiler { source, parser }
            })
            .collect();

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
