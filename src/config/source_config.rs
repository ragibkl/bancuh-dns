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
pub struct Source<T> {
    pub format: T,
    pub path: String,
}

pub type Blacklist = Source<BlacklistFormat>;
pub type Whitelist = Source<WhitelistFormat>;
pub type Overrides = Source<OverrideFormat>;

#[derive(serde::Deserialize, Debug, Clone)]
pub struct SourceConfig {
    pub blacklist: Vec<Blacklist>,
    pub whitelist: Vec<Whitelist>,
    pub overrides: Vec<Overrides>,
}
