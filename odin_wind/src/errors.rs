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
use odin_gdal::gdal::errors::GdalError;
use crate::actor::AddWindClient;

pub type Result<T> = std::result::Result<T, OdinWindError>;

#[derive(Error,Debug)]
pub enum OdinWindError {

    #[error("config error {0}")]
    ConfigError( #[from] odin_build::OdinBuildError),

    #[error("region name already in use {0:?}")]
    RegionInUseError(AddWindClient),

    #[error("invalid region coordinates {0:?}")]
    InvalidRegionError(AddWindClient),

    #[error("max number of regions exceeded {0:?}")]
    MaxRegionsExceeded(AddWindClient),

    #[error("internal DEM error {0}")]
    DemError(String),

    #[error("actor error {0}")]
    ActorError( #[from] odin_actor::OdinActorError),

    #[error("action failure {0}")]
    ActionFailure(String), // ActionFailure cannot impl std::error::Error

    #[error("Odin GDAL error {0}")]
    OdinGdalError( #[from] odin_gdal::errors::OdinGdalError),

    #[error("gdal error {0}")]
    GdalError( #[from] GdalError),

    #[error("IO error {0}")]
    IOError( #[from] std::io::Error),

    #[error("execution failed {0}")]
    ExecError(String),

    #[error("operation failed {0}")]
    OpFailedError(String)
}