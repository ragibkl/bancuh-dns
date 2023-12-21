mod file_or_url;
mod raw_config;
mod source;

use thiserror::Error;

pub use self::file_or_url::FileOrUrl;
pub use self::raw_config::{BlacklistFormat, OverrideFormat, WhitelistFormat};
pub use self::source::{BlacklistSource, OverridesSource, Source, WhitelistSource};

use crate::fetch::FetchError;

use self::{raw_config::RawConfig, source::FromRawSourceError};

#[derive(Debug, Clone)]
pub struct Config {
    pub config_url: FileOrUrl,
    pub blacklist: Vec<BlacklistSource>,
    pub whitelist: Vec<WhitelistSource>,
    pub overrides: Vec<OverridesSource>,
}

#[derive(Error, Debug)]
pub enum LoadConfigError {
    #[error("FetchError: {0}")]
    Fetch(#[from] FetchError),

    #[error("YamlError: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("FromSourceError: {0}")]
    FromSource(#[from] FromRawSourceError),
}

impl Config {
    fn try_from_raw_config(
        config_url: &FileOrUrl,
        source_config: &RawConfig,
    ) -> Result<Self, FromRawSourceError> {
        let mut blacklist: Vec<BlacklistSource> = Vec::new();

        for raw_source in &source_config.blacklist {
            let source = BlacklistSource::try_from_raw_source(config_url, raw_source)?;
            blacklist.push(source);
        }

        let mut whitelist: Vec<WhitelistSource> = Vec::new();
        for raw_source in &source_config.whitelist {
            let source = WhitelistSource::try_from_raw_source(config_url, raw_source)?;
            whitelist.push(source);
        }

        let mut overrides: Vec<OverridesSource> = Vec::new();
        for raw_source in &source_config.overrides {
            let source: Source<OverrideFormat> =
                OverridesSource::try_from_raw_source(config_url, raw_source)?;
            overrides.push(source);
        }

        Ok(Self {
            config_url: config_url.clone(),
            blacklist,
            whitelist,
            overrides,
        })
    }

    pub async fn load(config_url: &FileOrUrl) -> Result<Self, LoadConfigError> {
        let content = config_url.to_fetch().fetch().await?;
        let raw_config: RawConfig = serde_yaml::from_str(&content)?;
        let config = Self::try_from_raw_config(config_url, &raw_config)?;

        Ok(config)
    }
}
