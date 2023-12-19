use std::{fmt::Display, path::PathBuf, str::FromStr};

use thiserror::Error;
use url::Url;

#[derive(Clone, Debug)]
pub enum ConfigUrl {
    Url(Url),
    File(PathBuf),
}

#[derive(Error, Debug)]
#[error("ParseConfigError")]
pub struct ParseConfigError;

impl FromStr for ConfigUrl {
    type Err = ParseConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(url) = Url::parse(s) {
            return Ok(Self::Url(url));
        }

        if let Ok(path_buf) = PathBuf::from_str(s) {
            return Ok(Self::File(path_buf));
        }

        Err(ParseConfigError)
    }
}

impl Display for ConfigUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigUrl::Url(url) => f.write_fmt(format_args!("Url: {}", url)),
            ConfigUrl::File(path) => f.write_fmt(format_args!(
                "File: {}",
                path.as_path().as_os_str().to_str().unwrap_or_default()
            )),
        }
    }
}
