use crate::config::{FileOrUrl, WhitelistFormat, WhitelistSource};

use super::parser::{CName, Domain, Host};

#[derive(Debug, Clone)]
pub enum ParseWhitelist {
    Hosts,
    Domains,
    Zone,
}

impl ParseWhitelist {
    pub fn parse(&self, value: &str) -> Option<Domain> {
        match self {
            ParseWhitelist::Hosts => Host::parse(value).map(|h| h.into_domain()),
            ParseWhitelist::Domains => Domain::parse(value),
            ParseWhitelist::Zone => CName::parse(value).map(|c| c.into_domain()),
        }
    }
}

impl From<&WhitelistFormat> for ParseWhitelist {
    fn from(value: &WhitelistFormat) -> Self {
        match value {
            WhitelistFormat::Hosts => ParseWhitelist::Hosts,
            WhitelistFormat::Domains => ParseWhitelist::Domains,
            WhitelistFormat::Zone => ParseWhitelist::Zone,
        }
    }
}

#[derive(Debug)]
pub struct WhitelistCompiler {
    pub(crate) source: FileOrUrl,
    pub(crate) parser: ParseWhitelist,
}

impl WhitelistCompiler {
    pub async fn load_whitelist(&self) -> Vec<Domain> {
        let source = match self.source.to_fetch().fetch().await {
            Ok(s) => s,
            Err(err) => {
                println!("Could not fetch from {:?}", &self.source);
                println!("{}", err);
                println!("Skipping");
                return Vec::new();
            }
        };

        let parser = self.parser.clone();
        tokio::task::spawn_blocking(move || {
            source.lines().filter_map(|l| parser.parse(l)).collect()
        })
        .await
        .unwrap_or_default()
    }
}

impl From<&WhitelistSource> for WhitelistCompiler {
    fn from(wl: &WhitelistSource) -> Self {
        Self {
            source: wl.file_or_url.clone(),
            parser: ParseWhitelist::from(&wl.format),
        }
    }
}
