use crate::config::BlacklistFormat;

use super::{
    fetch_source::FetchSource,
    parser::{Domain, Host},
};

#[derive(Debug)]
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
    pub(crate) source: FetchSource,
    pub(crate) parser: ParseBlacklist,
}

impl BlacklistCompiler {
    pub async fn load_blacklist(&self) -> Vec<Domain> {
        let source = match self.source.fetch().await {
            Ok(s) => s,
            Err(err) => {
                println!("Could not fetch from {:?}", &self.source);
                println!("{}", err);
                println!("Skipping");
                return Vec::new();
            }
        };

        let mut domains: Vec<Domain> = Vec::new();
        for line in source.lines() {
            if let Some(bl) = self.parser.parse(line) {
                domains.push(bl);
            }
        }

        domains
    }
}
