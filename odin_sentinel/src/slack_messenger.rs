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

use std::{fs,path::{Path,PathBuf},collections::HashMap};
use reqwest;
use serde::{Serialize,Deserialize};
use async_trait::async_trait;

use odin_common::slack::{self,FileAttachment};
use crate::{op_failed, Alarm, AlarmMessenger, EvidenceInfo, OdinSentinelError};
use crate::errors::Result;

#[derive(Deserialize,Serialize,Debug)]
pub struct SlackAlarmConfig {
    token: String,
    device_channel_ids: HashMap<String,Vec<String>>,
    default_channel_ids: Vec<String>
}

impl SlackAlarmConfig {
    fn get_channel_ids (&self, device_id: &str)->&Vec<String> {
        self.device_channel_ids.get(device_id).unwrap_or( &self.default_channel_ids)
    }
}

/// Slack API based messenger for Sentinel Alarm notifications
pub struct SlackAlarmMessenger {
    config: SlackAlarmConfig,
}

impl SlackAlarmMessenger {
    pub fn new(config: SlackAlarmConfig)->Self { SlackAlarmMessenger{config} }
}

#[async_trait]
impl AlarmMessenger for SlackAlarmMessenger {

    async fn send_alarm (&self, alarm: &Alarm)->Result<()> {
        let config = &self.config;

        let files = get_file_attachments(alarm);
        if files.is_empty() {
            for channel_id in config.get_channel_ids( &alarm.device_id) {
                slack::send_msg( &config.token, channel_id, &alarm.description, None).await.map_err(|e| 
                    op_failed(e.to_string())
                )?;
            }
        } else {
            for channel_id in config.get_channel_ids( &alarm.device_id) {
                slack::send_msg_with_files( &config.token, channel_id, &alarm.description, &files).await.map_err(|e| 
                    op_failed(e.to_string())
                )?;
            }
        }

        Ok(())
    }
}

fn get_file_attachments (alarm: &Alarm)->Vec<FileAttachment> {
    let mut attachments: Vec<FileAttachment> = Vec::new();

    for e in &alarm.evidence_info {
        if let Some(sentinel_file) = &e.img {
            if sentinel_file.pathname.is_file() { 
                attachments.push( FileAttachment{ path: sentinel_file.pathname.clone(), caption: e.description.clone()})
            }
        }
    }

    attachments
}