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
use tokio::process::{Command,Child};
//use std::process::Command;
use which::which;
use odin_common::if_let;
use crate::{Alarm,AlarmMessenger,EvidenceInfo, OdinSentinelError};
use crate::errors::{op_failed,Result};

#[derive(Deserialize,Serialize)]
pub struct SignalCmdConfig {
    pub cmd: String,
    pub recipients: Vec<String>,
    pub group_ids: Vec<String>,
}

/// `AlarmMessenger` implementation that send alarms as text messages to Signal accounts
/// this requires a [`signal-cli`](https://github.com/AsamK/signal-cli) executable to be installed on
/// the local machine. Availability of such a command is checked when constructing the SignalCmsAlarmMessenger
/// and panics if none is found.
/// This messenger is always included and doesn't require odin_sentinel features.
/// Note this is using a different config than [`SignalRpcAlarmMessenger``]
pub struct SignalCmdAlarmMessenger {
    config: SignalCmdConfig,
}

impl SignalCmdAlarmMessenger {
    pub fn new (config: SignalCmdConfig)->Self {
        which(&config.cmd).expect( format!("unable to locate signal command {}", config.cmd).as_str()); // panic Ok - this is a toplevel object

        SignalCmdAlarmMessenger { config }
    }
}

impl AlarmMessenger for SignalCmdAlarmMessenger {
    async fn send_alarm (&self, alarm: Alarm)->Result<()> {
        let config = &self.config;
        let message = alarm.description;

        let attachments: Vec<&PathBuf> = alarm.evidence_info.iter().fold( Vec::<&PathBuf>::new(), |mut acc, e|{
            if let Some(sentinel_file) = &e.img {
                if sentinel_file.pathname.is_file() { 
                    acc.push( &sentinel_file.pathname) 
                }
            }
            acc
        });

        let mut usernames: Vec<&String> = Vec::new();
        let mut recipients: Vec<&String> = Vec::new();
        for r in &config.recipients {
            if r.starts_with("+") { recipients.push(r) } else { usernames.push(r) }
        }

        let mut cmd = Command::new( config.cmd.as_str());

        cmd
            .arg("send")
            .arg("--notify-self");

        if !attachments.is_empty() {
            cmd.arg("-a");
            for a in attachments { cmd.arg( a.as_os_str()); }
        }

        if !usernames.is_empty() {
            cmd.arg("-u");
            for u in usernames { cmd.arg(u); }
        }

        cmd
            .arg("-m")
            .arg( message);

        if !recipients.is_empty() {
            for r in recipients { cmd.arg(r); }
        }

        //println!("executing {cmd:?}");
        
        match cmd.spawn() {
            Ok(mut child) => {
                println!("executing {child:?}");
                match child.wait().await {
                    Ok(exit_status) =>  exit_status.exit_ok().map_err( |e| OdinSentinelError::CommandError(e.to_string())),
                    Err(e) => Err( OdinSentinelError::CommandError(e.to_string()) )
                }
            }
            Err(e) => Err( OdinSentinelError::CommandError(e.to_string()) )
        }
    }
}