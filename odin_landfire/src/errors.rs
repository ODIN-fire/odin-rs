/*
 * Copyright 2026 Peter Mehlitz
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *    http://apache.org
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use thiserror::Error;

pub type Result<T> = std::result::Result<T,OdinLandfireError>;

#[derive(Error,Debug)]
pub enum OdinLandfireError {
    #[error("file not found error {0}")]
    FileNotFoundError(String),

    #[error("IO error {0}")]
    IOError( #[from] std::io::Error),

    #[error("config RON error {0}")]
    RonError( #[from] ron::error::Error),

    #[error("GDAL error {0}")]
    GdalError( #[from] odin_gdal::errors::OdinGdalError),

    #[error("operation failed error {0}")]
    OpFailedError(String)
}

macro_rules! op_failed {
    ($fmt:literal $(, $arg:expr )* ) => {
        OdinLandfireError::OpFailedError( format!( $fmt $(, $arg)* ))
    };
}
pub (crate) use op_failed;
