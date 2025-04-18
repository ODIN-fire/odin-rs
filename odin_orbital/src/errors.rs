/*
 * Copyright © 2025, United States Government, as represented by the Administrator of 
 * the National Aeronautics and Space Administration. All rights reserved.
 *
 * The “ODIN” software is licensed under the Apache License, Version 2.0 (the "License"); 
 * you may not use this file except in compliance with the License. You may obtain a copy 
 * of the License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software distributed under
 * the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND,
 * either express or implied. See the License for the specific language governing permissions
 * and limitations under the License.
 */

use odin_actor::OdinActionFailure;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, OdinOrbitalError>;
 
#[derive(Error,Debug)]
pub enum OdinOrbitalError {

    #[error("Config error {0}")]
    ConfigError( #[from] odin_build::OdinBuildError),

    #[error("TLE error {0}")]
    TleError( String ),

    #[error("IO error {0}")]
    IOError( #[from] std::io::Error),

    #[error("http error {0}")]
    HttpError( #[from] odin_common::net::OdinNetError),
   
    #[error("Propagation error {0}")]
    Sgp4Error( String ),

    #[error("csv error {0}")]
    CsvError( #[from] csv::Error),

    #[error("scheduling error {0}")]
    ScheduleError( #[from] odin_job::OdinJobError),

    #[error("action error {0}")]
    ActionError( String ),

    #[error("operation failed {0}")]
    OpFailedError(String),
}

// OdinActionFailure is not a std::error::Error so we have to convert explicitly (see odin_action::OdinActionFailure)
impl From<OdinActionFailure> for OdinOrbitalError {
    fn from (e: OdinActionFailure)->OdinOrbitalError {
        OdinOrbitalError::ActionError(e.to_string())
    }
}

macro_rules! tle_error {
    ($fmt:literal $(, $arg:expr )* ) => {
        OdinOrbitalError::TleError( format!( $fmt $(, $arg)* ))
    };
}
pub (crate) use tle_error;

macro_rules! op_failed {
    ($fmt:literal $(, $arg:expr )* ) => {
        OdinOrbitalError::OpFailedError( format!( $fmt $(, $arg)* ))
    };
}
pub (crate) use op_failed;

macro_rules! action_failed {
    ($fmt:literal $(, $arg:expr )* ) => {
        OdinOrbitalError::ActionError( format!( $fmt $(, $arg)* ))
    };
}
pub (crate) use action_failed;