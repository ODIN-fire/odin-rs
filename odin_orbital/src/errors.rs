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

pub type Result<T> = std::result::Result<T, OdinOrbitalError>;
 
#[derive(Error,Debug)]
pub enum OdinOrbitalError {

   #[error("TLE error {0}")]
   TleError( String ),

   #[error("IO error {0}")]
   IOError( #[from] std::io::Error),

   #[error("http error {0}")]
   HttpError( #[from] reqwest::Error),
   
   #[error("Propagation error {0}")]
   Sgp4Error( String ),

   #[error("operation failed {0}")]
   OpFailedError(String),
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
