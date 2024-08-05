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

use std::collections::{VecDeque,HashMap};
use std::{time::Duration,sync::Arc,future::Future, path::PathBuf, io::Write};
use futures::SinkExt;
use odin_common::sim_clock;
use odin_common::{datetime::Dated,sim_clock::now,fs::{append_open,append_to_file,append_line_to_file}};
use serde::{Deserialize,Serialize,Serializer};
use serde_json;
use chrono::{DateTime, Local, TimeDelta, Utc};
use async_trait::async_trait;
use odin_actor::prelude::*;
use odin_macro::{match_algebraic_type, define_struct};
use uom::si::f32::Time;

use crate::{op_failed, sentinel_cache_dir, FireData, GetSentinelFile, RecordDataBounds, RecordRef, SensorRecord, SentinelFile, SentinelStore, SentinelUpdate, SmokeData};
use crate::actor::{SentinelActorMsg,GetSentinelUpdate};
use crate::errors::{OdinSentinelError, Result};

/// abstract alarm data
#[derive(Debug)]
pub struct Alarm {
    pub device_id: String,
    pub description: String,
    pub time_recorded: DateTime<Utc>,
    pub evidence_info: Vec<EvidenceInfo>,
}

/// abstract data to describe an evidence record
#[derive(Debug)]
pub struct EvidenceInfo {
    pub description: String,
    pub img: Option<SentinelFile>,
}

/// abstract interface for messenger services (SMS< Signal, WhatsApp etc)
/// since this is a simple interface that is hopefully not called too often we use `async_trait`` to
/// make it object-safe
#[async_trait]
pub trait AlarmMessenger: Send + Sync {
    /// impls have to make sure this is guaranteed to return in bounded time so that we know if notifications were sent out
    async fn send_alarm (&self, alarm: &Alarm)->Result<()>;
}

#[macro_export]
macro_rules! create_messengers {
    ( $( $msgr:expr ),* ) => {
        vec![
            $( Box::new($msgr) ),*
        ]
    }
}

/* #region SentinelAlarm ***************************************************************************************/

#[derive(Deserialize,Serialize,Debug)]
#[serde(default)]
pub struct SentinelAlarmMonitorConfig {
    new_alarm_duration: Duration, // after which we consider this to be a new alarm
    attach_image: bool,
    image_timeout: Duration,
    fire_prob: f64,
    smoke_prob: f64,
    old_alarm_duration: Duration, // after which we purge a stored alarm, needs to be > new_alarm_duration
    device_infos: HashMap<String,String>
}

impl Default for SentinelAlarmMonitorConfig {
    fn default()->Self {
        SentinelAlarmMonitorConfig {
            new_alarm_duration: minutes(10),
            attach_image: true,
            image_timeout: Duration::from_secs(20),
            fire_prob: 0.7,
            smoke_prob: 0.7,
            old_alarm_duration: Duration::from_mins(60),
            device_infos: HashMap::new()
        }
    }
}

const ALARM_HISTORY: usize = 10;

define_actor_msg_set! { pub SentinelAlarmMonitorMsg = SentinelUpdate | Alarm }

/// the Sentinel Alarm Actor state
define_struct! { pub SentinelAlarmMonitor =
    config: SentinelAlarmMonitorConfig,
    hupdater: ActorHandle<SentinelActorMsg>,
    messengers: Vec<Box<dyn AlarmMessenger>>,

    reported_fire_alarms: VecDeque<Arc<SensorRecord<FireData>>> = VecDeque::with_capacity( ALARM_HISTORY),
    reported_smoke_alarms: VecDeque<Arc<SensorRecord<SmokeData>>> = VecDeque::with_capacity( ALARM_HISTORY)
}

impl SentinelAlarmMonitor {

    fn check_new_alarm<T> (rec: &Arc<SensorRecord<T>>, reported_alarms: &mut VecDeque<Arc<SensorRecord<T>>>, config: &SentinelAlarmMonitorConfig) -> Option<String> 
        where T: RecordDataBounds 
    {
        // Ok to panic if there is no sim_clock or the config is inconsistent (but should happen sooner?)
        let now = Utc::now(); //sim_clock::now().unwrap(); 

        //--- clean up first
        let max_age = TimeDelta::from_std(config.old_alarm_duration).unwrap();
        while let Some(back) = reported_alarms.back() {
            if now - back.date() > max_age {
                reported_alarms.pop_back();
            } else {
                break
            }
        }

        //--- add it if new
        if !Self::is_reported_alarm(rec, reported_alarms, config.new_alarm_duration) {
            reported_alarms.push_front( rec.clone());
            Some(format!("{}({},{})", rec.capability().property_name(), rec.device_id, rec.time_recorded.format("%Y-%m-%dT%H:%M:%S%Z")))
        } else {
            None
        }
    }

    fn is_reported_alarm<T> (rec: &SensorRecord<T>, reported_alarms: &VecDeque<Arc<SensorRecord<T>>>, new_alarm_dur: Duration) -> bool where T: RecordDataBounds {
        for ref alarm in reported_alarms {
            if (alarm.device_id == rec.device_id) && (alarm.sensor_no == rec.sensor_no) {
                // shall we base this on last (not first) reported time? Maybe we should keep a list here
                let td = rec.time_recorded.signed_duration_since( alarm.time_recorded);
                return (td < TimeDelta::zero()) || (td.to_std().unwrap() < new_alarm_dur)
            } 
        }
        false
    }

    async fn collect_evidence_info (hupdater: &ActorHandle<SentinelActorMsg>, evidences: &Vec<RecordRef>, img_timeout: Duration) -> Result<Vec<EvidenceInfo>> {
        let mut evidence_info: Vec<EvidenceInfo> = Vec::new();

        for r in evidences {
            let record_id = r.id.clone();
            match timeout_query_ref( hupdater, GetSentinelUpdate {record_id}, secs(2)).await { // if we don't already have the record something is wrong
                Ok(Ok(upd)) => {  // the successful query response itself is a Result since the updater might not have the record 
                    let description = format!("sensor: {}", upd.sensor_no()); //upd.description();
                    let mut img: Option<SentinelFile> = None;

                    match_algebraic_type! { upd: SentinelUpdate as
                        Arc<SensorRecord<ImageData>> => {
                            let record_id = upd.id.clone();
                            let filename = upd.data.filename.clone();

                            img = match timeout_query_ref( hupdater, GetSentinelFile{record_id,filename}, img_timeout).await {
                                Ok(Ok(sentinel_file)) => Some(sentinel_file),
                                _ => { return Err( OdinSentinelError::FileRequestError(upd.data.filename.clone())) }
                            }
                        }
                        _ => {
                            // TODO - not interested in other evidence records?
                            warn!("ignoring non-image evidence record: {:?}", r.id);
                        } 
                    }

                    evidence_info.push( EvidenceInfo{description,img})
                }
                _ => {
                    return Err( OdinSentinelError::RecordRequestError( format!("failed to retrieve evidence record: {}", r.id)) )
                }
                
            }
        }

        Ok(evidence_info)
    }

    async fn process_fire_alarm (&mut self, hself: ActorHandle<SentinelAlarmMonitorMsg>, rec: Arc<SensorRecord<FireData>>) {
        if rec.data.fire_prob >= self.config.fire_prob {
            let reported_alarms = &mut self.reported_fire_alarms;
            if let Some(alarm_id) = Self::check_new_alarm( &rec, reported_alarms, &self.config) {
                let info: &str = self.device_info(&rec.device_id).map(|s|s.as_str()).unwrap_or("");
                let descr = format!("🔥 {}\ndevice: {} {}\nfire probability: {}", 
                    rec.time_recorded.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S %Z"), rec.device_id, info, rec.data.fire_prob);
                self.process_alarm( hself, &alarm_id, rec.device_id.clone(), descr, rec.time_recorded, &rec.evidences).await;
            }
        }
    }

    async fn process_smoke_alarm (&mut self, hself: ActorHandle<SentinelAlarmMonitorMsg>, rec: Arc<SensorRecord<SmokeData>>) {
        if rec.data.smoke_prob >= self.config.smoke_prob {
            let reported_alarms = &mut self.reported_smoke_alarms;
            if let Some(alarm_id) = Self::check_new_alarm( &rec, reported_alarms, &self.config) { // could use 💨 here but most fires cause smoke alarms
                let info: &str = self.device_info(&rec.device_id).map(|s|s.as_str()).unwrap_or("");
                let descr = format!("🔥 {}\ndevice: {} {}\nsmoke probability: {}", 
                    rec.time_recorded.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S %Z"), rec.device_id, info, rec.data.smoke_prob);
                self.process_alarm( hself, &alarm_id, rec.device_id.clone(), descr, rec.time_recorded, &rec.evidences).await;
            }
        }   
    }

    fn device_info(&self, device_id: &str)->Option<&String> {
        self.config.device_infos.get(device_id)
    }

    async fn process_alarm (&self, hself: ActorHandle<SentinelAlarmMonitorMsg>, alarm_id: &str, device_id: String, description: String, time_recorded: DateTime<Utc>, evidences: &Vec<RecordRef>) {
        self.log_alarm( alarm_id, &description, evidences);

        if !self.config.attach_image || evidences.is_empty() {  // send right away
            hself.send_msg( Alarm { device_id, description, time_recorded, evidence_info: Vec::with_capacity(0) }).await;

        } else { // we have to dig up the evidence image
            let hupdater = self.hupdater.clone();

            match Self::collect_evidence_info( &hupdater, evidences, self.config.image_timeout).await {
                Ok(evidence_info) => {
                    hself.send_msg( Alarm { device_id, description, time_recorded, evidence_info }).await;
                }
                Err(e) => {
                    warn!("failed to retrieve evidence for alarm {}", alarm_id);
                    hself.send_msg( Alarm { device_id, description, time_recorded, evidence_info: Vec::with_capacity(0) }).await;
                }
            };
        }
    }

    fn log_alarm (&self, alarm_descr: &str, description: &str, evidences: &Vec<RecordRef>) {
        let path = sentinel_cache_dir().join("alarm.log");
        match append_open(path) {
            Ok(mut file) => { writeln!(file, "{}: {}", Local::now(), alarm_descr); }
            Err(e) => { error!("failed to append to alarm.log: {:?}", e) }
        };
    }
}

impl_actor! { match msg for Actor<SentinelAlarmMonitor,SentinelAlarmMonitorMsg> as
    SentinelUpdate => cont! { // external - update notification
        let hself = self.hself.clone();
        match_algebraic_type! { msg: SentinelUpdate as 
            Arc<SensorRecord<FireData>> => self.process_fire_alarm( hself, msg).await,
            Arc<SensorRecord<SmokeData>> => self.process_smoke_alarm( hself, msg).await,
            _ => {} // not a record we are interested in
        }
    }
    Alarm => cont! { // internal message that we have to send out notifications  
        for msgr in &self.messengers {
            if let Err(e) = msgr.send_alarm( &msg).await {
                warn!("failed to send alarm notification: {e}");
            }
        }
    }
}

/* #endregion SentinelAlarm */

/* #region Messenger *****************************************************************************************/

/// this is just a dummy Messenger that prints out alarms to the console (used for testing)
pub struct ConsoleAlarmMessenger {}

#[async_trait]
impl AlarmMessenger for ConsoleAlarmMessenger {
    async fn send_alarm (&self, alarm: &Alarm)->Result<()> {
        //println!("ALARM: {alarm:?}");
        println!("{} {}", Local::now(), alarm.description);
        Ok(())
    }
}

/* #endregion Messenger */