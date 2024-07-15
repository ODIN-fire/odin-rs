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
use ron;
use cargo_toml;

pub type Result<T> = std::result::Result<T, OdinBuildError>;

#[derive(Error,Debug)]
pub enum OdinBuildError {
    #[error("IO error {0}")]
    IOError( #[from] std::io::Error),

    #[error("config RON error {0}")]
    RonError( #[from] ron::Error),

    #[error("config serialize/deserialize RON error {0}")]
    RonSerdeError( #[from] ron::error::SpannedError),

    #[error("Manifest error {0}")]
    ManifestError( #[from] cargo_toml::Error),

    #[error("env var error: {0}")]
    VarError( #[from] std::env::VarError),

    #[error("converting to utf8 string {0}")]
    Utf8Error( #[from] std::str::Utf8Error),

    #[error("minification failed: {0}")]
    MinifyError(String),

    #[error("resource not found {0}")]
    ResourceNotFoundError(String),

    #[error("unknown resource type {0}")]
    ResourceTypeError(String),
}

pub fn var_error()->OdinBuildError {
    OdinBuildError::VarError(std::env::VarError::NotPresent)
}