use crate::config::{BlacklistFormat, BlacklistSource, FileOrUrl};

use super::parser::{Domain, Host};

#[derive(Debug, Clone)]
pub enum ParseBlacklist {
    Hosts,
    Domains,
}

impl ParseBlacklist {
    pub fn parse(&self, value: &str) -> Option<Domain> {
        match self {
            ParseBlacklist::Hosts => Host::parse(value).map(|h| h.into_domain()),
            ParseBlacklist::Domains => Domain::parse(value),
        }
    }
}

impl From<&BlacklistFormat> for ParseBlacklist {
    fn from(value: &BlacklistFormat) -> Self {
        match value {
            BlacklistFormat::Hosts => ParseBlacklist::Hosts,
            BlacklistFormat::Domains => ParseBlacklist::Domains,
        }
    }
}

#[derive(Debug)]
pub struct BlacklistCompiler {
    pub(crate) source: FileOrUrl,
    pub(crate) parser: ParseBlacklist,
}

impl BlacklistCompiler {
    pub async fn load_blacklist(&self) -> Vec<Domain> {
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

impl From<&BlacklistSource> for BlacklistCompiler {
    fn from(bl: &BlacklistSource) -> Self {
        Self {
            source: bl.file_or_url.clone(),
            parser: ParseBlacklist::from(&bl.format),
        }
    }
}
