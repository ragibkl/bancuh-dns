use std::path::PathBuf;
use std::str::FromStr;

use thiserror::Error;
use url::Url;

use crate::{
    config::FileOrUrl,
    fetch::{Fetch, FetchError},
};

#[derive(Debug)]
pub struct FetchSource(pub Fetch);

#[derive(Error, Debug)]
#[error("FetchSourceError: {0}")]
pub struct FetchSourceError(#[from] FetchError);

#[derive(Error, Debug)]
pub enum FetchSourceInitError {
    #[error("InvalidUrl: {0}")]
    InvalidUrl(#[from] url::ParseError),

    #[error("InvalidPath")]
    InvalidPath,

    #[error("FileNotExists")]
    FileNotExists,

    #[error("Infallible")]
    Infallible(#[from] core::convert::Infallible),
}

impl FetchSource {
    pub fn try_from(
        source_path: &str,
        config_url: &FileOrUrl,
    ) -> Result<Self, FetchSourceInitError> {
        if source_path.starts_with("http") {
            let url = Url::parse(source_path)?;
            Ok(Self(url.into()))
        } else if source_path.starts_with("./") {
            match config_url {
                FileOrUrl::Url(u) => {
                    let url = u.join(source_path)?;
                    Ok(Self(url.into()))
                }
                FileOrUrl::File(p) => {
                    let path = p
                        .parent()
                        .ok_or(FetchSourceInitError::InvalidPath)?
                        .join(source_path);
                    if path.exists() {
                        Ok(Self(path.into()))
                    } else {
                        Err(FetchSourceInitError::FileNotExists)
                    }
                }
            }
        } else {
            let path = PathBuf::from_str(source_path)?;
            if path.exists() {
                Ok(Self(path.into()))
            } else {
                Err(FetchSourceInitError::FileNotExists)
            }
        }
    }

    pub async fn fetch(&self) -> Result<String, FetchSourceError> {
        Ok(self.0.fetch().await?)
    }
}
