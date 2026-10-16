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

#![allow(unused)]

use thiserror::Error;
use odin_build;
use odin_openmeteo;

pub type Result<T> = std::result::Result<T, OdinNfdrsError>;

#[derive(Error,Debug)]
pub enum OdinNfdrsError {
    #[error("build error {0}")]
    BuildError( #[from] odin_build::OdinBuildError),

    #[error("IO error {0}")]
    IOError( #[from] std::io::Error),

    #[error("Utf8Error {0}")]
    Utf8Error( #[from] std::str::Utf8Error),

    #[error("ParseIntError {0}")]
    ParseIntError( #[from] std::num::ParseIntError),

    #[error("ParseFloatError {0}")]
    ParseFloatError( #[from] std::num::ParseFloatError),

    #[error("net error {0}")]
    NetError( #[from] odin_common::net::OdinNetError),

    #[error("UTM error {0}")]
    UtmError( #[from] odin_common::utm::OdinUtmError),

    #[error("DEM error {0}")]
    DemError( #[from] odin_dem::errors::OdinDemError),

    #[error("OpenMeteo error {0}")]
    OpenMeteoError( #[from] odin_openmeteo::errors::OdinOpenMeteoError),

    #[error("Landfire error {0}")]
    LandfireError( #[from] odin_landfire::errors::OdinLandfireError),

    #[error("GDAL error {0}")]
    GdalError( #[from] odin_gdal::errors::OdinGdalError),

    #[error("CSV error {0}")]
    CsvError( #[from] csv::Error),

    #[error("actor error {0}")]
    ActorError( #[from] odin_actor::errors::OdinActorError),

    #[error("reqwest error {0}")]
    ReqwestError( #[from] reqwest::Error),

    #[error("serde error {0}")]
    SerdeError( #[from] serde_json::Error),

    #[error("config RON error {0}")]
    RonError( #[from] ron::error::Error),

    #[error("config chrono error {0}")]
    ChronoError( #[from] chrono::OutOfRangeError),

    #[error("chrono parse error {0}")]
    ChronoParseError( #[from] chrono::ParseError),

    #[error("action failure {0}")]
    ActionFailure(String), // ActionFailure cannot impl std::error::Error

    #[error("operation failed {0}")]
    OpFailedError(String),
}

macro_rules! op_failed {
    ($fmt:literal $(, $arg:expr )* ) => {
        OdinNfdrsError::OpFailedError( format!( $fmt $(, $arg)* ))
    };
}
pub (crate) use op_failed;
