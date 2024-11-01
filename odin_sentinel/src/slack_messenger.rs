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

#[derive(Deserialize,Debug)]
pub struct SlackAlarmConfig {
    pub token: String,
    pub alarm_channels: Vec<SlackAlarmChannel> 
}

/// the channel an alarm should be sent to, including optional filter values for device id, alarm type and minimum confidence
/// we keep this here as a flat struct so that it can be extended with format specifiers and alarm specific actions.
#[derive(Deserialize,Debug)]
pub struct SlackAlarmChannel {
    /// the Slack channel ID
    pub id: String,

    #[serde(default="default_device")]
    pub device: String,

    #[serde(default="default_alarm")]
    pub alarm: String,

    #[serde(default="default_min_confidence")]
    pub min_confidence: f64
}

fn default_device()->String { "*".into() } // all devices
fn default_alarm()->String { "*".into() }  // all alarm types
fn default_min_confidence()->f64 { 0.0 }   // all confidence values 

impl SlackAlarmChannel {
    pub fn matches (&self, alarm: &Alarm) -> bool {
        (self.device == "*" || alarm.device_id.starts_with( &self.device))
        && (self.alarm == "*" || alarm.alarm_type.starts_with( &self.alarm))
        && (alarm.confidence >= self.min_confidence)
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
            for alarm_channel in &self.config.alarm_channels {
                if alarm_channel.matches(alarm) {
                    slack::send_msg( &config.token, &alarm_channel.id, &alarm.description, None).await?;
                }
            }
        } else {
            for alarm_channel in &self.config.alarm_channels {
                if alarm_channel.matches(alarm) {
                    slack::send_msg_with_files( &config.token, &alarm_channel.id, &alarm.description, &files).await?;
                }
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