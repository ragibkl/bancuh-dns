use crate::config::OverrideFormat;

use super::{fetch_source::FetchSource, parser::CName};

#[derive(Debug)]
pub(super) enum ParseRewrite {
    CName,
}

impl ParseRewrite {
    fn parse(&self, value: &str) -> Option<CName> {
        match self {
            ParseRewrite::CName => CName::parse(value),
        }
    }
}

impl From<&OverrideFormat> for ParseRewrite {
    fn from(value: &OverrideFormat) -> Self {
        match value {
            OverrideFormat::Cname => ParseRewrite::CName,
        }
    }
}

#[derive(Debug)]
pub struct RewritesCompiler {
    pub(super) source: FetchSource,
    pub(super) parser: ParseRewrite,
}

impl RewritesCompiler {
    pub async fn load_rewrites(&self) -> Vec<CName> {
        let source = match self.source.fetch().await {
            Ok(s) => s,
            Err(err) => {
                println!("Could not fetch from {:?}", &self.source);
                println!("{}", err);
                println!("Skipping");
                return Vec::new();
            }
        };

        let mut cnames: Vec<CName> = Vec::new();
        for line in source.lines() {
            if let Some(bl) = self.parser.parse(line) {
                cnames.push(bl);
            }
        }

        cnames
    }
}
