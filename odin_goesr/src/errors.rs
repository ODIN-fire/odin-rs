/*
 * Copyright (c) 2024, United States Government, as represented by the
 * Administrator of the National Aeronautics and Space Administration.
 * All rights reserved.
 *
 * The ODIN - Open Data Integration Framework is licensed under the
 * Apache License, Version 2.0 (the "License"); you may not use this file
 * except in compliance with the License. You may obtain a copy of the
 * License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use thiserror::Error;

pub type Result<T> = std::result::Result<T, OdinGoesRError>;

#[derive(Error,Debug)]
pub enum OdinGoesRError {

    #[error("build error {0}")]
    BuildError( #[from] odin_build::OdinBuildError),

    #[error("S3 error {0}")]
    S3Error( #[from] odin_common::s3::OdinS3Error),

    #[error("IO error {0}")]
    IOError( #[from] std::io::Error),

    #[error("time delta out of range error {0}")]
    DurationError( #[from] chrono::OutOfRangeError),

    #[error("No object error")]
    NoObjectError( String ),

    #[error("No object key error")]
    NoObjectKeyError(),

    #[error("NetCDF data set error: {0}")]
    DatasetError( String ),

    #[error("No object date error")]
    NoObjectDateError(),

    #[error("String to float conversion error {0}")]
    FloatConversionError( #[from] std::num::ParseFloatError),

    #[error("invalid filename")]
    FilenameError(String),

    #[error("Misc error {0}")]
    MiscError( String ),

    #[error("serde error {0}")]
    SerdeError( #[from] serde_json::Error),

    #[error("ODIN Actor error {0}")]
    OdinActorError( #[from] odin_actor::errors::OdinActorError),

    #[error("ODIN GDAL error {0}")]
    OdinGdalError( #[from] odin_gdal::errors::OdinGdalError),

    #[error("ODIN GDAL error {0}")]
    GdalError( #[from] odin_gdal::errors::GdalError)
}

pub fn misc_error (msg: impl ToString)->OdinGoesRError {
    OdinGoesRError::MiscError(msg.to_string())
}

pub fn no_object_error (msg: impl ToString)->OdinGoesRError {
    OdinGoesRError::NoObjectError(msg.to_string())
}

pub fn filename_error (msg: impl ToString)->OdinGoesRError {
    OdinGoesRError::FilenameError(msg.to_string())
}