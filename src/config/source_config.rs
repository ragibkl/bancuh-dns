#[derive(serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum BlacklistFormat {
    Hosts,
    Domains,
}

#[derive(serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum WhitelistFormat {
    Hosts,
    Domains,
    Zone,
}

#[derive(serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum OverrideFormat {
    Cname,
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct Source<T: Clone> {
    pub format: T,
    pub path: String,
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct SourceConfig {
    pub blacklist: Vec<Source<BlacklistFormat>>,
    pub whitelist: Vec<Source<WhitelistFormat>>,
    pub overrides: Vec<Source<OverrideFormat>>,
}
