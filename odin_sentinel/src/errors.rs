/*
 * Copyright (c) 2024, United States Government, as represented by the
 * Administrator of the National Aeronautics and Space Administration.
 * All rights reserved.
 *
 * The ODIN - Open Data Integration Framework is licensed under the
 * Apache License, Version 2.0 (the "License"); you may not use this file
 * except in compliance with the License. You may obtain a copy of the
 * License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use thiserror::Error;
use odin_actor::errors::OdinActorError;
use ron;

pub type Result<T> = std::result::Result<T, OdinSentinelError>;

#[derive(Error,Debug)]
pub enum OdinSentinelError {
    #[error("IO error {0}")]
    IOError( #[from] std::io::Error),

    #[error("config parse error {0}")]
    ConfigParseError(String),

    #[error("http error {0}")]
    HttpError( #[from] reqwest::Error),

    #[error("http error {0}")]
    HttpError1( #[from] tokio_tungstenite::tungstenite::http::Error),

    #[error("http header error {0}")]
    HttpHeaderError( #[from] tokio_tungstenite::tungstenite::http::header::InvalidHeaderValue),

    #[error("URL parse error (0)")]
    UrlParseError( #[from] url::ParseError),

    #[error("websock error {0}")]
    WsError( #[from] tokio_tungstenite::tungstenite::Error),

    #[error("websock protocol error {0}")] 
    WsProtocolError(String), // unexpected/wrong responses

    #[error("websocket closed by server")]
    WsClosedError,

    #[error("actor error {0}")]
    ActorError( #[from] OdinActorError),

    #[error("JSON error {0}")]
    JsonError( #[from] serde_json::Error),

    #[error("RON error {0}")]
    RonError( #[from] ron::error::Error),

    #[error("no data error {0}")]
    NoDataError(String),

    #[error("no such device error {0}")]
    NoSuchDeviceError(String),

    #[error("no devices")]
    NoDevicesError,

    #[error("error retrieving file {0}")]
    FileRequestError(String),

    // ...add specific errors here

    /// a generic error
    #[error("operation failed {0}")]
    OpFailed(String)
}

pub fn no_data (msg: impl ToString)->OdinSentinelError {
    OdinSentinelError::NoDataError(msg.to_string())
}

pub fn op_failed (msg: impl ToString)->OdinSentinelError {
    OdinSentinelError::OpFailed(msg.to_string())
}

