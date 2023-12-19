mod file;
mod http;

use std::path::PathBuf;

use thiserror::Error;
use url::Url;

use self::{
    file::{FetchFile, FetchFileError},
    http::{FetchHTTPError, FetchHttp},
};

#[derive(Debug)]
pub enum Fetch {
    Http(FetchHttp),
    File(FetchFile),
}

#[derive(Error, Debug)]
pub enum FetchError {
    #[error("HTTPError: {0}")]
    HTTPError(#[from] FetchHTTPError),

    #[error("FileError: {0}")]
    FileError(#[from] FetchFileError),
}

impl Fetch {
    pub async fn fetch(&self) -> Result<String, FetchError> {
        match self {
            Fetch::Http(p) => Ok(p.fetch().await?),
            Fetch::File(p) => Ok(p.fetch().await?),
        }
    }
}

impl From<Url> for Fetch {
    fn from(url: Url) -> Self {
        Self::Http(FetchHttp { url })
    }
}

impl From<PathBuf> for Fetch {
    fn from(path: PathBuf) -> Self {
        Self::File(FetchFile { path })
    }
}
