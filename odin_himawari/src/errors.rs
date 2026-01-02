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

use thiserror::Error;
use csv;
use suppaftp;
use odin_build;
use odin_actor;
use odin_dem;


pub type Result<T> = std::result::Result<T, OdinHimawariError>;

#[derive(Error,Debug)]
pub enum OdinHimawariError {

    #[error("build error {0}")]
    BuildError( #[from] odin_build::OdinBuildError),

    #[error("IO error {0}")]
    IOError( #[from] std::io::Error),

    #[error("serde error {0}")]
    SerdeError( #[from] serde_json::Error),

    #[error("ftp error {0}")]
    FtpError( #[from] suppaftp::FtpError),

    #[error("csv error {0}")]
    CsvError( #[from] csv::Error),

    #[error("action error {0}")]
    ActionError( String ),

    #[error("actor error {0}")]
    ActorError( #[from] odin_actor::OdinActorError ),

    #[error("ODIN DEM error {0}")]
    OdinDemError( #[from] odin_dem::errors::OdinDemError),

    #[error("operation failed {0}")]
    OpFailedError(String),
}

macro_rules! op_failed {
    ($fmt:literal $(, $arg:expr )* ) => {
        OdinHimawariError::OpFailedError( format!( $fmt $(, $arg)* ))
    };
}
pub (crate) use op_failed;
