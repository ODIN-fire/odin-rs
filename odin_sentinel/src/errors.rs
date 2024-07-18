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
use odin_actor::errors::OdinActorError;
use odin_common::map_to_opaque_error;
use odin_job::OdinJobError;
use ron;

pub type Result<T> = std::result::Result<T, OdinSentinelError>;

/// odin_sentinel specific error type. Note that we need those to be Clone, hence we use
/// our own mapping into opaque types that do not store the source error
#[derive(Error,Debug,Clone)]
pub enum OdinSentinelError {
    #[error("IO error {0}")]
    IOError(String),

    #[error("config error {0}")]
    ConfigError(String),

    #[error("http error {0}")]
    HttpError(String),

    #[error("websock error {0}")]
    WsError(String),

    #[error("websock protocol error {0}")] 
    WsProtocolError(String), // unexpected/wrong responses

    #[error("websocket closed by server")]
    WsClosedError,

    #[error("actor error {0}")]
    ActorError(String),

    #[error("connector error {0}")]
    ConnectorError(String),

    #[error("job error {0}")]
    JobError(String),

    #[error("JSON error {0}")]
    JsonError(String),

    #[error("no data error {0}")]
    NoDataError(String),

    #[error("no such device error {0}")]
    NoSuchDeviceError(String),

    #[error("no such record error {0}")]
    NoSuchRecordError(String),

    #[error("no devices")]
    NoDevicesError,

    #[error("error retrieving file {0}")]
    FileRequestError(String),

    #[error("command error {0}")]
    CommandError(String),

    #[error("smtp error {0}")]
    SmtpError(String),

    #[error("rpc error {0}")]
    RpcError(String),

    #[error("timeout error {0}")]
    TimeoutError(String),

    // ...add specific errors here

    /// a generic error
    #[error("operation failed {0}")]
    OpFailed(String)
}

map_to_opaque_error!{ std::io::Error => OdinSentinelError::IOError }
map_to_opaque_error!{ serde_json::Error => OdinSentinelError::JsonError }
map_to_opaque_error!{ reqwest::Error => OdinSentinelError::HttpError }
map_to_opaque_error!{ tokio_tungstenite::tungstenite::http::Error => OdinSentinelError::HttpError }
map_to_opaque_error!{ tokio_tungstenite::tungstenite::http::header::InvalidHeaderValue => OdinSentinelError::HttpError }
map_to_opaque_error!{ url::ParseError => OdinSentinelError::HttpError }
map_to_opaque_error!{ tokio_tungstenite::tungstenite::Error => OdinSentinelError::WsError }
map_to_opaque_error!{ odin_actor::errors::OdinActorError => OdinSentinelError::ActorError }
map_to_opaque_error!{ odin_job::OdinJobError => OdinSentinelError::JobError }
map_to_opaque_error!{ ron::error::Error => OdinSentinelError::ConfigError }
map_to_opaque_error!{ tokio::time::error::Elapsed => OdinSentinelError::TimeoutError }
map_to_opaque_error!{ std::process::ExitStatusError => OdinSentinelError::CommandError }

#[cfg(feature="smtp")]
map_to_opaque_error!{ lettre::transport::smtp::Error => OdinSentinelError::SmtpError }

#[cfg(feature="signal_rpc")]
map_to_opaque_error!{ jsonrpsee::core::client::Error => OdinSentinelError::RpcError }


pub fn no_data (msg: impl ToString)->OdinSentinelError {
    OdinSentinelError::NoDataError(msg.to_string())
}

pub fn op_failed (msg: impl ToString)->OdinSentinelError {
    OdinSentinelError::OpFailed(msg.to_string())
}

pub fn connector_error (msg: impl ToString)->OdinSentinelError {
    OdinSentinelError::ConnectorError(msg.to_string())
}

pub fn send_error (msg: impl ToString)->OdinSentinelError {
    OdinSentinelError::ConnectorError(msg.to_string())
}

#[macro_export]
macro_rules! op_failed {
    ($fmt:literal $(, $arg:expr )* ) => {
        op_failed( format!( $fmt $(, $arg)* ))
    };
}
