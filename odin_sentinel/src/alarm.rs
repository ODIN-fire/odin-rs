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
use std::{time::Duration,sync::Arc,future::Future, path::PathBuf, io::Write as IoWrite, fmt::Write as FmtWrite};
use futures::SinkExt;
use odin_common::fs::get_filename_extension;
use odin_common::geo::DatedGeoPos;
use odin_common::sim_clock;
use odin_common::{datetime::Dated,sim_clock::now,fs::{append_open,append_to_file,append_line_to_file}};
use serde::{Deserialize,Serialize,Serializer};
use serde_json;
use chrono::{DateTime, Local, TimeDelta, Utc};
use async_trait::async_trait;
use odin_actor::prelude::*;
use odin_macro::{match_algebraic_type, define_struct};
use uom::si::f32::Time;

use crate::{op_failed, sentinel_cache_dir, ExternalImage, FireData, GetSentinelFile, GetSentinelPosition, RecordDataBounds, RecordRef, SensorRecord, SentinelDeviceInfo, SentinelDeviceInfos, SentinelFile, SentinelStore, SentinelUpdate, SmokeData
};
use crate::actor::{SentinelActorMsg,GetSentinelUpdate};
use crate::errors::{OdinSentinelError, Result};

/// abstract alarm data
#[derive(Debug)]
pub struct Alarm {
    pub device_id: String,
    pub description: String,
    pub time_recorded: DateTime<Utc>,
    pub pos: Option<DatedGeoPos>,
    pub alarm_type: String,
    pub confidence: f64,
    pub evidence_info: Vec<EvidenceInfo>,
}

/// abstract data to describe an evidence record
#[derive(Debug,Clone)]
pub struct EvidenceInfo {
    pub sensor_no: u32, // sensor this evidence was associated with
    pub description: String,
    pub img: Option<SentinelFile>,
}

// check if two EvidenceInfo Vecs are synonymous (they might differ in order)
fn same_evidence_sensors (ev1: &Vec<EvidenceInfo>, ev2: &Vec<EvidenceInfo>)->bool {
    if ev1.len() != ev2.len() { return false }

    let mut n_matches = 0;
    for a in ev1 {
        for b in ev2 {
           if a.sensor_no == b.sensor_no { n_matches += 1 }
        }
    }
    n_matches == ev1.len()
}

/// match spec that can be used in messengers to choose actions to take for a given alarm
#[derive(Debug,Serialize,Deserialize)]
#[serde(default)]
pub struct AlarmMatcher {
    pub device: String,
    pub alarm: String,
    pub min_confidence: f64
}

impl AlarmMatcher {
    pub fn matches (&self, alarm: &Alarm) -> bool {
        (self.device == "*" || alarm.device_id.starts_with( &self.device))
        && (self.alarm == "*" || alarm.alarm_type.starts_with( &self.alarm))
        && (alarm.confidence >= self.min_confidence)
    }
}

impl Default for AlarmMatcher {
    fn default() -> Self {
        Self { 
            device: "*".into(), 
            alarm: "*".into(), 
            min_confidence: 0.0 
        }
    }
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
    pub new_alarm_duration: Duration, // after which we consider this to be a new alarm. Zero means every alarm is reported
    pub old_alarm_duration: Duration, // after which we purge a stored alarm, needs to be > new_alarm_duration

    pub attach_image: bool,
    pub image_timeout: Duration,
    pub fire_prob: f64,
    pub smoke_prob: f64,
}

impl Default for SentinelAlarmMonitorConfig {
    fn default()->Self {
        SentinelAlarmMonitorConfig {
            new_alarm_duration: minutes(10),
            old_alarm_duration: Duration::from_mins(60),
            attach_image: true,
            image_timeout: Duration::from_secs(20),
            fire_prob: 0.7,
            smoke_prob: 0.7,
        }
    }
}

/// for now this is just a cache so that we don't have to retrieve EvidenceInfos on each check
/// but we could add more context info here
struct ReportedAlarm<T> where T: RecordDataBounds{
    rec: Arc<SensorRecord<T>>,
    evidence_info: Vec<EvidenceInfo>
}

const ALARM_HISTORY: usize = 10;

define_actor_msg_set! { pub SentinelAlarmMonitorMsg = SentinelUpdate | Alarm }

/// the Sentinel Alarm Actor state
define_struct! { pub SentinelAlarmMonitor =
    config: SentinelAlarmMonitorConfig,
    device_infos: SentinelDeviceInfos,
    hupdater: ActorHandle<SentinelActorMsg>,
    messengers: Vec<Box<dyn AlarmMessenger>>,

    reported_fire_alarms: VecDeque<ReportedAlarm<FireData>> = VecDeque::with_capacity( ALARM_HISTORY),
    reported_smoke_alarms: VecDeque<ReportedAlarm<SmokeData>> = VecDeque::with_capacity( ALARM_HISTORY)
}

impl SentinelAlarmMonitor {

    async fn process_fire_alarm (&mut self, hself: ActorHandle<SentinelAlarmMonitorMsg>, rec: Arc<SensorRecord<FireData>>) {
        if rec.data.fire_prob >= self.config.fire_prob {
            let mut evidence_info = self.retrieve_evidence( &self.hupdater, &rec.evidences, self.config.image_timeout).await;

            let reported_alarms = &mut self.reported_fire_alarms;
            if let Some(alarm_id) = Self::check_new_alarm( &rec, &evidence_info, reported_alarms, &self.config) {
                let info: &str = self.device_infos.get(&rec.device_id).map(|s|s.name.as_str()).unwrap_or("");
                let descr = format!("🔥 {}\ndevice: {} {}\nfire probability: {}", 
                    rec.time_recorded.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S %Z"), rec.device_id, info, rec.data.fire_prob);
                let alarm_type = rec.capability().property_name().to_string();
                let confidence = rec.data.fire_prob;
                self.process_alarm( hself, &alarm_id, &rec.id, rec.device_id.clone(), descr, rec.time_recorded, alarm_type, confidence, evidence_info).await;
            }
        }
    }

    async fn process_smoke_alarm (&mut self, hself: ActorHandle<SentinelAlarmMonitorMsg>, rec: Arc<SensorRecord<SmokeData>>) {
        if rec.data.smoke_prob >= self.config.smoke_prob {
            let mut evidence_info = self.retrieve_evidence( &self.hupdater, &rec.evidences, self.config.image_timeout).await;

            let reported_alarms = &mut self.reported_smoke_alarms;
            if let Some(alarm_id) = Self::check_new_alarm( &rec, &evidence_info, reported_alarms, &self.config) { // could use 💨 here but most fires cause smoke alarms
                let info: &str = self.device_infos.get(&rec.device_id).map(|s|s.name.as_str()).unwrap_or("");
                let descr = format!("🔥 {}\ndevice: {} {}\nsmoke probability: {}", 
                    rec.time_recorded.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S %Z"), rec.device_id, info, rec.data.smoke_prob);
                let alarm_type = rec.capability().property_name().to_string();
                let confidence = rec.data.smoke_prob;
                self.process_alarm( hself, &alarm_id, &rec.id, rec.device_id.clone(), descr, rec.time_recorded, alarm_type, confidence, evidence_info).await;
            }
        }   
    }

    fn check_new_alarm<T> (rec: &Arc<SensorRecord<T>>, evidence: &Vec<EvidenceInfo>, reported_alarms: &mut VecDeque<ReportedAlarm<T>>, config: &SentinelAlarmMonitorConfig) -> Option<String> 
        where T: RecordDataBounds 
    {
        if config.new_alarm_duration.is_zero() { // every alarm is treated as a new one - no need to store ReportedAlarms
            Some(format!("{}({},{})", rec.capability().property_name(), rec.device_id, rec.time_recorded.format("%Y-%m-%dT%H:%M:%S%Z")))

        } else {
            // Ok to panic if there is no sim_clock or the config is inconsistent (but should happen sooner?)
            let now = Utc::now(); //sim_clock::now().unwrap(); 

            //--- clean up old alarms first
            let max_age = TimeDelta::from_std(config.old_alarm_duration).unwrap();
            reported_alarms.retain_mut( |alarm| now - alarm.rec.date() < max_age);

            //--- add it if new
            if !Self::is_reported_alarm(rec, evidence, reported_alarms, config.new_alarm_duration) {
                let new_alarm = ReportedAlarm { rec: rec.clone(), evidence_info: evidence.clone() };
                reported_alarms.push_front( new_alarm);
                Some(format!("{}({},{})", rec.capability().property_name(), rec.device_id, rec.time_recorded.format("%Y-%m-%dT%H:%M:%S%Z")))
            } else {
                None
            }
        }
    }

    fn is_reported_alarm<T> (rec: &SensorRecord<T>, evidence: &Vec<EvidenceInfo>, reported_alarms: &VecDeque<ReportedAlarm<T>>, new_alarm_dur: Duration) -> bool where T: RecordDataBounds {
        for ref alarm in reported_alarms {
            // we count a differing evidence as a new alarm, no matter of how old. This is essential so that we don't
            // treat alarms by different cameras of the same device as the same alarm
            if (alarm.rec.device_id == rec.device_id) && (alarm.rec.sensor_no == rec.sensor_no) && same_evidence_sensors(evidence, &alarm.evidence_info){
                // shall we base this on last (not first) reported time? Maybe we should keep a list here
                let td = rec.time_recorded.signed_duration_since( alarm.rec.time_recorded);
                return (td < TimeDelta::zero()) || (td.to_std().unwrap() < new_alarm_dur)
            } 
        }
        false
    }

    async fn process_alarm (&self, hself: ActorHandle<SentinelAlarmMonitorMsg>, 
        alarm_id: &str, record_id: &str, device_id: String, 
        mut description: String, time_recorded: DateTime<Utc>, alarm_type: String, confidence: f64, mut evidence_info: Vec<EvidenceInfo>
    ) 
    {
        self.log_alarm( alarm_id, &description, &evidence_info);
        let hupdater = &self.hupdater;
        let pos = self.retrieve_pos( hupdater, &device_id, time_recorded).await;
        if let Some(p) = pos {
            let alt = 180000.0; // [m] - we could use p.alt + x here
            write!( description, "\nhttps://wildfireai.com/odin-fire/live?view={:.4},{:.4},{:.0}", p.lat.degrees(), p.lon.degrees(), alt);
        }

        if !self.config.attach_image {  // we don't want images - send right away
            hself.send_msg( Alarm { device_id, description, time_recorded, pos, alarm_type, confidence, evidence_info: Vec::with_capacity(0) }).await;

        } else { // we have to dig up the evidence image(s)
            let timeout = self.config.image_timeout;

            if let Some(device_info) = self.device_infos.get(&device_id) {
                self.add_external_evidence( &mut evidence_info, device_info, hupdater, record_id, time_recorded, timeout).await;
            }

            hself.send_msg( Alarm { device_id, description, time_recorded, pos, alarm_type, confidence, evidence_info }).await;
        }
    }

    async fn retrieve_pos (&self, hupdater: &ActorHandle<SentinelActorMsg>, device_id: &String, date: DateTime<Utc>) -> Option<DatedGeoPos> {
        match timeout_query_ref( hupdater, GetSentinelPosition{device_id: device_id.clone(),date}, secs(2)).await {
            Ok(res) => res,
            _ => {
                warn!("failed to retrieve position for device {} at {}", device_id, date);
                None
            }
        }
    }

    async fn retrieve_evidence (&self, hupdater: &ActorHandle<SentinelActorMsg>, evidences: &Vec<RecordRef>, img_timeout: Duration) -> Vec<EvidenceInfo> {
        let mut evidence_info: Vec<EvidenceInfo> = Vec::with_capacity(2); // usually just one or two images

        // our own evidence - we have to get the filename from the respective evidence image record 
        for r in evidences {
            let record_id = r.id.clone();
            match timeout_query_ref( hupdater, GetSentinelUpdate {record_id}, secs(2)).await { // if we don't already have the record something is wrong
                Ok(Ok(upd)) => {  // the successful query response itself is a Result since the updater might not have the record 
                    let sensor_no = upd.sensor_no();
                    let description = format!("sensor: {sensor_no}"); //upd.description();

                    match_algebraic_type! { upd: SentinelUpdate as
                        Arc<SensorRecord<ImageData>> => {
                            let record_id = upd.id.clone();
                            //let filename = upd.data.filename.clone(); // @@@ remove 
                            let filename = upd.odin_filename();

                            match timeout_query_ref( hupdater, GetSentinelFile::internal(record_id, filename), img_timeout).await {
                                Ok(Ok(sentinel_file)) => {
                                    let img = Some(sentinel_file);
                                    evidence_info.push( EvidenceInfo{sensor_no, description, img})
                                }
                                _ => warn!("failed to retrieve evidence file {}", upd.odin_filename())
                            }
                        }
                        _ => warn!("ignoring non-image evidence record: {:?}", r.id)
                    }
                }
                _ => warn!("failed to retrieve evidence record: {}", r.id)                
            }
        }

        evidence_info
    }

    async fn add_external_evidence (&self, evidence_info: &mut Vec<EvidenceInfo>, device_info: &SentinelDeviceInfo,
                                    hupdater: &ActorHandle<SentinelActorMsg>, record_id: &str, time_recorded: DateTime<Utc>, timeout: Duration) {
        let sensors: Vec<u32> = evidence_info.iter().map( |ei| ei.sensor_no).collect();

        for ext_img in &device_info.external_images {
            if let Some(sensor_no) = sensors.iter().find( |s| ext_img.supports_sensor(**s)).map(|s| *s) {
                let uri = ext_img.uri().to_string();
                let description = uri.clone();
                let ext = get_filename_extension(&uri).unwrap_or("");
                let filename = format!("{}-{}.{}", 
                    time_recorded.format("%Y%m%d-%H%M%S_%3f"),
                    ext_img.filename(), 
                    ext
                );

                match timeout_query_ref( hupdater, GetSentinelFile::external(record_id.to_string(), filename, uri), timeout).await {
                    Ok(Ok(sentinel_file)) => {
                        let img = Some(sentinel_file);
                        evidence_info.push( EvidenceInfo{sensor_no, description, img});
                    }
                    _ => {
                        // external imagery is supposed to be supplemental - don't bail if we can't get it in time
                        // TODO - should we at least add the URI here ?
                    } 
                }
            }
        }
    }

    fn log_alarm (&self, alarm_descr: &str, description: &str, evidences: &Vec<EvidenceInfo>) {
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