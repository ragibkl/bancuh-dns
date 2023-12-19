use thiserror::Error;

use super::{
    fetch_config::{FetchConfig, FetchConfigError},
    Config, ConfigUrl, SourceConfig,
};

pub struct LoadConfig {
    config_url: ConfigUrl,
    fetch_config: FetchConfig,
}

#[derive(Error, Debug)]
pub enum LoadConfigError {
    #[error("FetchError: {0}")]
    Fetch(#[from] FetchConfigError),

    #[error("YamlError: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

impl LoadConfig {
    pub async fn load(&self) -> Result<Config, LoadConfigError> {
        let content = self.fetch_config.fetch().await?;
        let source_config: SourceConfig = serde_yaml::from_str(&content)?;

        Ok(Config {
            config_url: self.config_url.clone(),
            blacklist: source_config.blacklist,
            whitelist: source_config.whitelist,
            overrides: source_config.overrides,
        })
    }
}

impl From<&ConfigUrl> for LoadConfig {
    fn from(config_url: &ConfigUrl) -> Self {
        Self {
            config_url: config_url.clone(),
            fetch_config: config_url.into(),
        }
    }
}
