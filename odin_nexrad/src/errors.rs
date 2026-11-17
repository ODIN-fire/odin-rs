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

pub type Result<T> = std::result::Result<T,OdinNexradError>;

#[derive(Error,Debug)]
pub enum OdinNexradError {
    #[error("IO error {0}")]
    IOError( #[from] std::io::Error),

    #[error("S3 error {0}")]
    S3Error( #[from] odin_common::s3::OdinS3Error),

    #[error("NEXRAD error {0}")]
    NexradError( #[from] nexrad::Error),

    #[error("CSV error {0}")]
    CsvError( #[from] csv::Error),

    #[error("GDAL error {0}")]
    GdalError( #[from] gdal::errors::GdalError),

    #[error("odin_gdal error {0}")]
    OdinGdalError( #[from] odin_gdal::errors::OdinGdalError),

    #[error("actor error {0}")]
    ActorError( #[from] odin_actor::errors::OdinActorError),

    #[error("serde error {0}")]
    SerdeError( #[from] serde_json::Error),

    #[error("config RON error {0}")]
    RonError( #[from] ron::error::SpannedError),

    #[error("operation failed {0}")]
    OpFailedError(String)
}

macro_rules! op_failed {
    ($fmt:literal $(, $arg:expr )* ) => {
        crate::errors::OdinNexradError::OpFailedError( format!( $fmt $(, $arg)* ))
    };
}
pub (crate) use op_failed;
