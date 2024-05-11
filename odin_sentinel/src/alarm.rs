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

use std::{time::Duration,sync::Arc,future::Future};
use serde::{Deserialize,Serialize,Serializer};
use serde_json;
use chrono::TimeDelta;
use odin_actor::prelude::*;
use odin_macro::{match_algebraic_type, define_struct};

use crate::{SensorRecord,RecordRef,RecordDataBounds,FireData,SmokeData,SentinelStore,SentinelUpdate,SentinelFile,GetSentinelFile};
use crate::actor::{SentinelActorMsg,GetSentinelUpdate};
use crate::errors::Result;

/// abstract alarm data
#[derive(Debug)]
pub struct Alarm {
    pub description: String,
    pub evidence_info: Vec<EvidenceInfo>,
}

/// abstract data to describe an evidence record
#[derive(Debug)]
pub struct EvidenceInfo {
    pub description: String,
    pub img: Option<SentinelFile>,
}

/// abstract interface for messenger services (SMS< Signal, WhatsApp etc)
pub trait AlarmMessenger: Send {
    fn send_alarm (&self, alarm: Alarm)->impl Future<Output=Result<()>> + Send;
}

/* #region SentinelAlarm ***************************************************************************************/

#[derive(Deserialize,Serialize,Debug)]
#[serde(default)]
pub struct SentinelAlarmMonitorConfig {
    new_alarm_duration: Duration,
    attach_image: bool,
    fire_prob: f64,
    smoke_prob: f64,
}

impl Default for SentinelAlarmMonitorConfig {
    fn default()->Self {
        SentinelAlarmMonitorConfig {
            new_alarm_duration: minutes(10),
            attach_image: true,
            fire_prob: 0.5,
            smoke_prob: 0.5,
        }
    }
}

define_actor_msg_set! { pub SentinelAlarmMonitorMsg = SentinelUpdate | Alarm }

/// the Sentinel Alarm Actor state
define_struct! { pub SentinelAlarmMonitor<A> where A: AlarmMessenger =
    config: SentinelAlarmMonitorConfig,
    hupdater: ActorHandle<SentinelActorMsg>,
    messenger: A,

    reported_fire_alarms: Vec<Arc<SensorRecord<FireData>>> = Vec::new(),
    reported_smoke_alarms: Vec<Arc<SensorRecord<SmokeData>>> = Vec::new()
}

impl<A> SentinelAlarmMonitor<A> where A: AlarmMessenger {

    fn is_reported_alarm<T> (&self, rec: &SensorRecord<T>, reported_alarms: &Vec<Arc<SensorRecord<T>>>) -> bool where T: RecordDataBounds {
        for ref alarm in reported_alarms {
            if (alarm.device_id == rec.device_id) && (alarm.sensor_no == rec.sensor_no) {
                // shall we base this on last (not first) reported time? Maybe we should keep a list here
                let td = rec.time_recorded.signed_duration_since( alarm.time_recorded);
                return (td < TimeDelta::zero()) || (td.to_std().unwrap() < self.config.new_alarm_duration)
            } 
        }
        false
    }

    async fn collect_evidence_info (hupdater: &ActorHandle<SentinelActorMsg>, evidences: &Vec<RecordRef>)->Vec<EvidenceInfo> {
        let mut evidence_info: Vec<EvidenceInfo> = Vec::new();

        for r in evidences {
            let record_id = r.id.clone();
            match timeout_query_ref( hupdater, GetSentinelUpdate {record_id}, secs(1)).await {
                Ok(Ok(upd)) => {  // the successful query response itself is a Result since the updater might not have the record 
                    let description = upd.description();
                    let mut img: Option<SentinelFile> = None;

                    match_algebraic_type! { upd: SentinelUpdate as
                        Arc<SensorRecord<ImageData>> => {
                            let record_id = upd.id.clone();
                            let filename = upd.data.filename.clone();

                            img = match timeout_query_ref( hupdater, GetSentinelFile{record_id,filename}, secs(5)).await {
                                Ok(Ok(sentinel_file)) => Some(sentinel_file),
                                _ => { error!("failed to retrieve evidence image {}", upd.data.filename); None }
                            }
                        }
                        _ => {} // TODO - not interested in other evidence records?
                    }

                    evidence_info.push( EvidenceInfo{description,img})
                }
                _ => error!("failed to retrieve evidence record {}", r.id)
            }
        }

        evidence_info
    }

    async fn process_fire (&mut self, hself: ActorHandle<SentinelAlarmMonitorMsg>, rec: Arc<SensorRecord<FireData>>) {
        if rec.data.fire_prob >= self.config.fire_prob {
            if !self.is_reported_alarm( &rec, &self.reported_fire_alarms) {
                self.reported_fire_alarms.push( rec.clone());
                let description = rec.description();

                if !self.config.attach_image || rec.evidences.is_empty() {  // send right away
                    hself.send_msg( Alarm { description, evidence_info: Vec::with_capacity(0) }).await;

                } else { // we have to dig up the evidence image
                    let hupdater = self.hupdater.clone();

                    spawn( "fire-alarm", async move { // needs to spawn since it might take a while
                        let evidence_info = Self::collect_evidence_info( &hupdater, &rec.evidences).await;
                        hself.send_msg( Alarm { description, evidence_info }).await;
                    });
                }
            }
        }
    }

    async fn process_smoke (&mut self, hself: ActorHandle<SentinelAlarmMonitorMsg>, rec: Arc<SensorRecord<SmokeData>>) {
        // TODO
    }
}

impl_actor! { match msg for Actor<SentinelAlarmMonitor<M>,SentinelAlarmMonitorMsg> where M: AlarmMessenger as
    SentinelUpdate => cont! { // external - update notification
        let hself = self.hself.clone();
        match_algebraic_type! { msg: SentinelUpdate as 
            Arc<SensorRecord<FireData>> => self.process_fire( hself, msg).await,
            Arc<SensorRecord<SmokeData>> => self.process_smoke( hself, msg).await,
            _ => {} // not a record we are interested in
        }
    }
    Alarm => cont! { // internal - send out alarm message through configured messenger
        self.messenger.send_alarm( msg).await
    }
}

/* #endregion SentinelAlarm */

/* #region Messenger *****************************************************************************************/

/// this is just a dummy Messenger that prints out alarms to the console (used for testing)
pub struct ConsoleAlarmMessenger {}

impl AlarmMessenger for ConsoleAlarmMessenger {
    async fn send_alarm (&self, alarm: Alarm)->Result<()> {
        println!("ALARM: {alarm:?}");
        Ok(())
    }
}

/* #endregion Messenger */