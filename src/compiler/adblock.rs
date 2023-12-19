use thiserror::Error;

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
    pub fn init(config: &Config) -> Result<Self, AdblockCompilerConfigError> {
        let mut blacklists = Vec::new();

        for bl in &config.blacklist {
            let source = FetchSource::try_from(&bl.path, &config.config_url)?;
            let parser = ParseBlacklist::from(&bl.format);

            blacklists.push(BlacklistCompiler { source, parser });
        }

        let mut whitelists = Vec::new();
        for wl in &config.whitelist {
            let source = FetchSource::try_from(&wl.path, &config.config_url)?;
            let parser = ParseWhitelist::from(&wl.format);

            whitelists.push(WhitelistCompiler { source, parser });
        }

        let mut rewrites = Vec::new();
        for rw in &config.overrides {
            let source = FetchSource::try_from(&rw.path, &config.config_url)?;
            let parser = ParseRewrite::from(&rw.format);

            rewrites.push(RewritesCompiler { source, parser });
        }

        Ok(Self {
            blacklists,
            whitelists,
            rewrites,
        })
    }

    pub async fn compile(&self, db: &AdblockDB) {
        for wl in &self.whitelists {
            let domains = wl.load_whitelist().await;
            for d in domains {
                db.insert_whitelist(&d.0);
            }
        }

        for bl in &self.blacklists {
            let domains = bl.load_blacklist().await;
            for d in domains {
                db.insert_blacklist(&d.0);
            }
        }

        for rw in &self.rewrites {
            let cnames = rw.load_rewrites().await;
            for c in cnames {
                db.insert_rewrite(&c.domain.0, &c.alias.0);
            }
        }
    }
}
