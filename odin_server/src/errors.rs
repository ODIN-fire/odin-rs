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

pub type OdinServerResult<T> = std::result::Result<T, OdinServerError>;
 
#[derive(Error,Debug)]
pub enum OdinServerError {

    #[error("ODIN Actor error {0}")]
    OdinActorError( #[from] odin_actor::errors::OdinActorError),

    #[error("build error: {0}")]
    OdinBuildError( #[from] odin_build::OdinBuildError),

    #[error("IO error: {0}")]
    IoError( #[from] std::io::Error),

    #[error("JSON error: {0}")]
    JsonError( #[from] serde_json::Error),

    #[error("unsupported resource: {0}")]
    UnsupportedResourceType(String),

    #[error("service init error: {0}")]
    ServiceInitError(String),

    #[error("connect error: {0}")]
    ConnectError(String),

    #[error("axum error: {0}")]
    AxumError( #[from] axum::Error),

    #[error("RON deserialization error {0}")]
    RonDeError( #[from] ron::de::SpannedError),

    #[error("operation failed: {0}")]
    OpFailed( String ),
}

pub fn op_failed (msg: impl ToString)->OdinServerError {
    OdinServerError::OpFailed(msg.to_string())
}

pub fn init_error (msg: impl ToString)->OdinServerError {
    OdinServerError::ServiceInitError(msg.to_string())
}

pub fn connect_error (msg: impl ToString)->OdinServerError {
    OdinServerError::ConnectError(msg.to_string())
}
