mod domain_source;
mod file_or_url;
// mod fetch_config;
// mod load_config;
mod source_config;

use thiserror::Error;

pub use self::domain_source::DomainSource;
pub use self::file_or_url::FileOrUrl;
pub use self::source_config::{BlacklistFormat, OverrideFormat, WhitelistFormat};

use crate::fetch::FetchError;

use self::{domain_source::FromSourceError, source_config::SourceConfig};

pub type Blacklist = DomainSource<BlacklistFormat>;
pub type Whitelist = DomainSource<WhitelistFormat>;
pub type Overrides = DomainSource<OverrideFormat>;

#[derive(Debug, Clone)]
pub struct Config {
    pub config_url: FileOrUrl,
    pub blacklist: Vec<Blacklist>,
    pub whitelist: Vec<Whitelist>,
    pub overrides: Vec<Overrides>,
}

#[derive(Error, Debug)]
pub enum LoadConfigError {
    #[error("FetchError: {0}")]
    Fetch(#[from] FetchError),

    #[error("YamlError: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("FromSourceError: {0}")]
    FromSource(#[from] FromSourceError),
}

impl Config {
    pub fn try_from_source_config(
        config_url: FileOrUrl,
        source_config: SourceConfig,
    ) -> Result<Self, FromSourceError> {
        let mut blacklist: Vec<Blacklist> = Vec::new();

        for bl in &source_config.blacklist {
            let bl = Blacklist::try_from_source(&config_url, bl)?;
            blacklist.push(bl);
        }

        let mut whitelist: Vec<Whitelist> = Vec::new();
        for wl in &source_config.whitelist {
            let wl = Whitelist::try_from_source(&config_url, wl)?;
            whitelist.push(wl);
        }

        let mut overrides: Vec<Overrides> = Vec::new();
        for rw in &source_config.overrides {
            let rw = Overrides::try_from_source(&config_url, rw)?;
            overrides.push(rw);
        }

        Ok(Self {
            config_url,
            blacklist,
            whitelist,
            overrides,
        })
    }

    pub async fn load(config_url: &FileOrUrl) -> Result<Self, LoadConfigError> {
        let content = config_url.to_fetch().fetch().await?;
        let source_config: SourceConfig = serde_yaml::from_str(&content)?;
        let config = Self::try_from_source_config(config_url.clone(), source_config)?;

        Ok(config)
    }
}
