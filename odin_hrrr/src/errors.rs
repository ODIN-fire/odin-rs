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
 use reqwest;

 pub type Result<T> = std::result::Result<T, OdinHrrrError>;

#[derive(Error,Debug)]
pub enum OdinHrrrError {
    #[error("IO error {0}")]
    IOError( #[from] std::io::Error),

    #[error("config error {0}")]
    ConfigError( #[from] odin_build::OdinBuildError),

    #[error("http error {0}")]
    HttpError( #[from] reqwest::Error),

    #[error("actor error {0}")]
    ActorError( #[from] odin_actor::OdinActorError),

    #[error("schedule error {0}")]
    ScheduleError(String),

    /// a generic error
    #[error("operation failed {0}")]
    OpFailed(String)
}

pub fn op_failed (msg: impl ToString)->OdinHrrrError {
    OdinHrrrError::OpFailed(msg.to_string())
}

pub fn schedule_error (msg: impl ToString)->OdinHrrrError {
    OdinHrrrError::ScheduleError(msg.to_string())
}