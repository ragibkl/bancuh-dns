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
pub struct RawSource<T: Clone> {
    pub format: T,
    pub path: String,
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct RawConfig {
    pub blacklist: Vec<RawSource<BlacklistFormat>>,
    pub whitelist: Vec<RawSource<WhitelistFormat>>,
    pub overrides: Vec<RawSource<OverrideFormat>>,
}
