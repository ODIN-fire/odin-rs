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

use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use jsonrpsee::proc_macros::rpc;
use jsonrpsee::core::client::{Error as RpcError, SubscriptionClientT};
use jsonrpsee::async_client::Client;
use jsonrpsee::http_client::{HttpClient, HttpClientBuilder};

use odin_common::if_let;
use crate::{Alarm,AlarmMessenger,EvidenceInfo};
use crate::errors::{op_failed,Result as SentinelResult};

#[derive(Deserialize,Serialize)]
pub struct SignalRpcConfig {
    pub signal_uri: String,
    pub signal_account: String,
    pub recipients: Vec<String>,
    pub group_ids: Vec<String>,
}

/// send-message RPC definition.
/// this expands into both a 'trait RpcClient' plus a 'impl<T> RpcClient for T where T: SubscriptionClient'
/// that is picked up by the HttpClient we are about to create.
/// see https://github.com/AsamK/signal-cli/blob/master/client/src/jsonrpc.rs
#[rpc(client)] 
pub trait Rpc { 
    #[serde(rename_all="camelCase")]
    #[method(name = "send", param_kind = map)]
    fn send(
        &self,
        account: Option<&String>,  // use ref since it directly comes from config
        recipients: &Vec<String>,  // also directly from config
        group_ids: &Vec<String>,   // also directly from config
        message: String,
        attachments:Vec<String>,
        notify_self: bool,  // TODO - as of signal-cli 0.13.4-SNAPSHOT this is only working when invoking the "send" command interactively
    ) -> Result<Value, ErrorObjectOwned>;
}

fn create_client (uri: &str)->HttpClient {
    match HttpClientBuilder::default().build(uri) {
        Ok(client) => client,
        Err(e) => panic!("Failed to connect to socket: {e}") // this is a toplevel-instantiated object so panic is Ok
    }
}

/// `AlarmMessenger` implementation that send alarms as text messages to Signal accounts
/// this requires a running [`signal-cli`](https://github.com/AsamK/signal-cli) server at the configured uri
/// (see [signal-cli man page](https://github.com/AsamK/signal-cli/blob/master/man/signal-cli.1.adoc))
pub struct SignalRpcAlarmMessenger {
    config: SignalRpcConfig,
    client: HttpClient
}

impl SignalRpcAlarmMessenger {
    pub fn new (config: SignalRpcConfig)->Self {
        let client = create_client( config.signal_uri.as_str());

        SignalRpcAlarmMessenger {
            config,
            client
        }
    }
}

impl AlarmMessenger for SignalRpcAlarmMessenger {
    async fn send_alarm (&self, alarm: Alarm)->SentinelResult<()> {
        let config = &self.config;
        let message = alarm.description;

        let mut attachments: Vec<String> = Vec::new();
        for e in alarm.evidence_info {
            if_let! {
                Some(sentinel_file) = e.img,
                Ok(pb) = sentinel_file.pathname.canonicalize(),
                Some(pn) = pb.to_str() => {
                    attachments.push(pn.into())
                }
            }
        }

        let res = self.client.send(
            Some(&config.signal_account),
            &config.recipients,
            &config.group_ids,
            message,
            attachments,
            true, // always notify self - it's an alarm
        ).await;

        match res {
            Ok(_) => Ok(()),
            Err(e) => Err(op_failed( format!("RPC send of Signal alarm failed: {e:?}")))
        }
    }
}