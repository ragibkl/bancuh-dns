use thiserror::Error;

use super::{
    file_or_url::ParseFileOrUrlError, raw_config::RawSource, BlacklistFormat, FileOrUrl,
    OverrideFormat, WhitelistFormat,
};

#[derive(Error, Debug)]
pub enum FromRawSourceError {
    #[error("InvalidUrl: {0}")]
    InvalidUrl(#[from] url::ParseError),

    #[error("InvalidPath")]
    InvalidPath,

    #[error("FileNotExists")]
    FileNotExists,

    #[error("Infallible")]
    Infallible(#[from] core::convert::Infallible),

    #[error("InvalidFileOrUrl")]
    InvalidFileOrUrl(#[from] ParseFileOrUrlError),
}

#[derive(Debug, Clone)]
pub struct Source<T: Clone> {
    pub format: T,
    pub file_or_url: FileOrUrl,
}

impl<T: Clone> Source<T> {
    pub fn try_from_raw_source(
        config_url: &FileOrUrl,
        source: &RawSource<T>,
    ) -> Result<Self, FromRawSourceError> {
        let file_or_url = if source.path.starts_with("./") {
            match config_url {
                FileOrUrl::Url(u) => FileOrUrl::Url(u.join(&source.path)?),
                FileOrUrl::File(p) => {
                    let path = p
                        .parent()
                        .ok_or(FromRawSourceError::InvalidPath)?
                        .join(&source.path);

                    if path.exists() {
                        FileOrUrl::File(path)
                    } else {
                        return Err(FromRawSourceError::FileNotExists);
                    }
                }
            }
        } else {
            source.path.parse()?
        };

        Ok(Self {
            format: source.format.clone(),
            file_or_url,
        })
    }
}

pub type BlacklistSource = Source<BlacklistFormat>;
pub type WhitelistSource = Source<WhitelistFormat>;
pub type OverridesSource = Source<OverrideFormat>;
