/*
 * Copyright © 2024, United States Government, as represented by the Administrator of 
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
use odin_gdal::errors::OdinGdalError;

#[derive(Error,Debug)]
pub enum OdinDemError {

    #[error("unsupported target spatial ref system: {0}")]
    UnsupportedTargetSRS(String),

    #[error("invalid filename: {0}")]
    FilenameError(String),

    // generic self-created error
    #[error("DEM operation failed: {0}")]
    OpFailedError(String),

    // pass through for IO errors
    #[error("DEM IO error: {0}")]
    IOError( #[from] std::io::Error),

    // pass through for OdinGdalErrors
    #[error("ODIN gdal error {0}")]
    OdinGdalError(#[from] OdinGdalError),

}

pub fn op_failed<S: ToString> (msg: S)->OdinDemError {
    OdinDemError::OpFailedError(msg.to_string())
}

pub fn invalid_filename<S: ToString> (fname: S)->OdinDemError {
    OdinDemError::FilenameError(fname.to_string())
}