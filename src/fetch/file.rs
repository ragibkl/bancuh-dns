use std::{fs::read_to_string, path::PathBuf};

use thiserror::Error;

#[derive(Debug)]
pub struct FetchFile {
    pub path: PathBuf,
}

#[derive(Error, Debug)]
pub enum FetchFileError {
    #[error("FileError: {0}")]
    FileError(#[from] std::io::Error),
}

impl FetchFile {
    pub async fn fetch(&self) -> Result<String, FetchFileError> {
        let contents = read_to_string(&self.path)?;
        Ok(contents)
    }
}
