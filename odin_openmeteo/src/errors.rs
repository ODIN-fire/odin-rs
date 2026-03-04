/*
 * Copyright © 2026, United States Government, as represented by the Administrator of
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
#![allow(unused)]

use thiserror::Error;
use odin_common;
use odin_actor;
use odin_gdal;
use odin_job;
use reqwest;
use serde_json;
use ron;
use gdal;
use chrono;

pub type Result<T> = std::result::Result<T,OdinOpenMetError>;

#[derive(Error,Debug)]
pub enum OdinOpenMetError {
    #[error("parse error {0}")]
    ParseError(String),

    #[error("IO error {0}")]
    IOError( #[from] std::io::Error),

    #[error("Utf8Error {0}")]
    Utf8Error( #[from] std::str::Utf8Error),

    #[error("net error {0}")]
    NetError( #[from] odin_common::net::OdinNetError),

    #[error("actor error {0}")]
    ActorError( #[from] odin_actor::errors::OdinActorError),

    #[error("reqwest error {0}")]
    ReqwestError( #[from] reqwest::Error),

    #[error("serde error {0}")]
    SerdeError( #[from] serde_json::Error),

    #[error("config RON error {0}")]
    RonError( #[from] ron::error::SpannedError),

    #[error("config chrono error {0}")]
    ChronoError( #[from] chrono::OutOfRangeError),

    #[error("chrono parse error {0}")]
    ChronoParseError( #[from] chrono::ParseError),

    #[error("GDAL error {0}")]
    GdalError( #[from] gdal::errors::GdalError),

    #[error("ODIN gdal error {0}")]
    OdinGdalError( #[from] odin_gdal::errors::OdinGdalError),

    #[error("ODIN wx error {0}")]
    OdinWxError( #[from] odin_wx::errors::OdinWxError),

    #[error("ODIN job error {0}")]
    OdinJobError( #[from] odin_job::OdinJobError),

    #[error("operation failed {0}")]
    OpFailedError(String)
}

macro_rules! parse_error {
    ($fmt:literal $(, $arg:expr )* ) => {
        crate::errors::OdinOpenMetError::ParseError( format!( $fmt $(, $arg)* ))
    };
}
pub (crate) use parse_error;

macro_rules! op_failed {
    ($fmt:literal $(, $arg:expr )* ) => {
        crate::errors::OdinOpenMetError::OpFailedError( format!( $fmt $(, $arg)* ))
    };
}
pub (crate) use op_failed;
