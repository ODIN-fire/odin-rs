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

#![allow(unused)]

use thiserror::Error;

 pub type Result<T> = std::result::Result<T,OdinN5Error>;


#[derive(Error,Debug)]
pub enum OdinN5Error {

    #[error("parse error {0}")]
    ParseError(String),

    #[error("IO error {0}")]
    IOError( #[from] std::io::Error),

    #[error("net error {0}")]
    NetError( #[from] odin_common::net::OdinNetError),
        
    #[error("header error {0}")]
    InvalidHeaderError( #[from] http::header::InvalidHeaderValue),

    #[error("reqwest error {0}")]
    ReqwestError( #[from] reqwest::Error),

    #[error("serde error {0}")]
    SerdeError( #[from] serde_json::Error),

    #[error("operation failed {0}")]
    OpFailedError(String)
}


macro_rules! parse_error {
    ($fmt:literal $(, $arg:expr )* ) => {
        OdinN5Error::ParseError( format!( $fmt $(, $arg)* ))
    };
}
pub (crate) use parse_error;