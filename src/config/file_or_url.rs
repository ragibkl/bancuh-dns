use std::{fmt::Display, path::PathBuf, str::FromStr};

use thiserror::Error;
use url::Url;

use crate::fetch::Fetch;

#[derive(Clone, Debug)]
pub enum FileOrUrl {
    Url(Url),
    File(PathBuf),
}

#[derive(Error, Debug)]
#[error("ParseFileOrUrlError")]
pub struct ParseFileOrUrlError;

impl FileOrUrl {
    pub fn to_fetch(&self) -> Fetch {
        match self {
            FileOrUrl::Url(url) => url.to_owned().into(),
            FileOrUrl::File(path) => path.to_owned().into(),
        }
    }
}

impl FromStr for FileOrUrl {
    type Err = ParseFileOrUrlError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(url) = Url::parse(s) {
            return Ok(Self::Url(url));
        }

        if let Ok(path_buf) = PathBuf::from_str(s) {
            return Ok(Self::File(path_buf));
        }

        Err(ParseFileOrUrlError)
    }
}

impl Display for FileOrUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileOrUrl::Url(url) => f.write_fmt(format_args!("Url: {}", url)),
            FileOrUrl::File(path) => f.write_fmt(format_args!(
                "File: {}",
                path.as_path().as_os_str().to_str().unwrap_or_default()
            )),
        }
    }
}
