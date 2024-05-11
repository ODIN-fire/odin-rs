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

use crate::{Alarm,AlarmMessenger,EvidenceInfo};
use crate::errors::{op_failed,Result as SentinelResult};

#[derive(Deserialize,Serialize)]
pub struct SignalConfig {
    pub signal_uri: String,
    pub signal_account: String,
    pub recipients: Option<Vec<String>>,
    pub group_ids: Option<Vec<String>>,
}

// send-message RPC definition.
// this expands into both a 'trait RpcClient' plus a 'impl<T> RpcClient for T where T: SubscriptionClient'
// that is picked up by the HttpClient we are about to create
#[rpc(client)] 
pub trait Rpc { 
    #[serde(rename_all="camelCase")]
    #[method(name = "send", param_kind = map)]
    fn send(
        &self,
        account: Option<&String>,
        recipients: &Option<Vec<String>>,
        group_ids: &Option<Vec<String>>,
        message: String,
        attachments:Option<Vec<PathBuf>>,
    ) -> Result<Value, ErrorObjectOwned>;
}

fn create_client (uri: &str)->HttpClient {
    match HttpClientBuilder::default().build(uri) {
        Ok(client) => client,
        Err(e) => panic!("Failed to connect to socket: {e}") // this is a toplevel-instantiated object so panic is Ok
    }
}

/// `AlarmMessenger` implementation that send alarms as text messages to Signal accounts
pub struct SignalAlarmMessenger {
    config: SignalConfig,
    client: HttpClient
}

impl SignalAlarmMessenger {
    pub fn new (config: SignalConfig)->Self {
        let client = create_client( config.signal_uri.as_str());

        SignalAlarmMessenger {
            config,
            client
        }
    }
}

impl AlarmMessenger for SignalAlarmMessenger {
    async fn send_alarm (&self, alarm: Alarm)->SentinelResult<()> {
        let config = &self.config;
        let message = alarm.description;

        let mut pb: Vec<PathBuf> = Vec::new();
        for e in alarm.evidence_info {
            if let Some(img) = e.img { pb.push( img.pathname) }
        }
        let attachments = if pb.is_empty() { None } else { Some(pb) };

        let res = self.client.send(
            Some(&config.signal_account),
            &config.recipients,
            &config.group_ids,
            message,
            attachments
        ).await;

        match res {
            Ok(_) => Ok(()),
            Err(e) => Err(op_failed( format!("RPC send of Signal alarm failed: {e:?}")))
        }
    }
}