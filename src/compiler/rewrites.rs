use crate::config::{FileOrUrl, OverrideFormat, OverridesSource};

use super::parser::CName;

#[derive(Debug, Clone)]
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
    pub(super) source: FileOrUrl,
    pub(super) parser: ParseRewrite,
}

impl RewritesCompiler {
    pub async fn load_rewrites(&self) -> Vec<CName> {
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

impl From<&OverridesSource> for RewritesCompiler {
    fn from(rw: &OverridesSource) -> Self {
        Self {
            source: rw.file_or_url.clone(),
            parser: ParseRewrite::from(&rw.format),
        }
    }
}
