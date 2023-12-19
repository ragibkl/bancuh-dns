mod config_url;
mod fetch_config;
mod load_config;
mod source_config;

pub use config_url::ConfigUrl;
pub use load_config::LoadConfig;
pub use source_config::*;

#[derive(Debug, Clone)]
pub struct Config {
    pub config_url: ConfigUrl,
    pub blacklist: Vec<Blacklist>,
    pub whitelist: Vec<Whitelist>,
    pub overrides: Vec<Overrides>,
}
