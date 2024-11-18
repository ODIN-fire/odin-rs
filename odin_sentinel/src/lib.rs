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
#![allow(unused)]
#![feature(trait_alias,exit_status_error,duration_constructors)]

use std::u64;
#[doc = include_str!("../doc/odin_sentinel.md")]

use std::{
    cmp::{min, Ordering}, collections::{HashMap, VecDeque}, fmt::{self,Debug}, 
    fs::File, future::Future, io::{Read, Write}, ops::RangeBounds, path::{Path,PathBuf}, 
    rc::Rc, sync::{atomic::{self,AtomicU64}, Arc}, time::Duration
};
use serde::{de::DeserializeOwned, Deserialize, Serialize, Serializer};
use serde_json;
use ron::{self, to_string};
use chrono::{DateTime,Utc};
use strum::IntoStaticStr;
use tokio_util::bytes::Buf;
use uom::si::f64::{Velocity,ThermodynamicTemperature,ElectricCurrent,ElectricPotential};
use reqwest::{Client,Response};
use async_trait::async_trait;
use paste::paste;
use lazy_static::lazy_static;

use odin_build::{define_load_asset, define_load_config};
use odin_common::{angle::{LatAngle, LonAngle, Angle},
    datetime::{Dated,deserialize_duration,to_epoch_millis},
    geo::DatedGeoPos,
    fs::{ensure_writable_dir, get_filename_extension}
};
use odin_actor::{MsgReceiver, Query, ActorHandle};
use odin_macro::{define_algebraic_type, match_algebraic_type, define_struct};

mod actor;
pub use actor::*;

mod alarm;
pub use alarm::*;

pub mod ws;

mod live_connector;
pub use live_connector::*;

mod errors;
pub use errors::*;

pub mod sentinel_service;

define_load_config!{}
define_load_asset!{}

//--- alarm messengers
mod signal_cmd; // this is always included
pub use signal_cmd::*;

#[cfg(feature="signal_rpc")] mod signal_rpc;
#[cfg(feature="signal_rpc")] pub use signal_rpc::*;

#[cfg(feature="smtp")] mod smtp;
#[cfg(feature="smtp")] pub use smtp::*;

#[cfg(feature="slack")] mod slack_messenger;
#[cfg(feature="slack")] pub use slack_messenger::*;

lazy_static! {
    static ref MSG_COUNTER: AtomicU64 = AtomicU64::new(42);
}

pub fn get_next_msg_id ()->String {
    MSG_COUNTER.fetch_add( 1, atomic::Ordering::Relaxed).to_string()
}

/* #region sensor record  ***************************************************************************/

pub trait CapabilityProvider {
    fn capability()->SensorCapability;
}

pub type DeviceId = String;
pub type RecordId = String;
pub trait RecordDataBounds = CapabilityProvider + Serialize + for<'de2> Deserialize<'de2> + Debug + Clone + 'static;

#[derive(Deserialize,Debug,Clone)]
#[serde(bound = "T: Serialize, for<'de2> T: Deserialize<'de2>")]
#[serde(rename_all="camelCase")]
pub struct SensorRecord <T> where T: RecordDataBounds {   
    pub id: RecordId, 

    pub time_recorded: DateTime<Utc>,
    pub sensor_no: u32,
    pub device_id: DeviceId,

    pub evidences: Vec<RecordRef>, 
    pub claims: Vec<RecordRef>,

    // here is the crux - we get this as different properties ("gps" etc - it depends on T)
    // since we need to preserve the mapping for subsequent serializing we have to provide alias annotations (for de)
    // *and* our own Serialize impl 
    #[serde(alias="accelerometer",alias="anemometer",alias="cloudcover",alias="event",alias="fire",alias="image",alias="gas",alias="gps",alias="gyroscope",
            alias="magnetometer",alias="orientation",alias="person",alias="power",alias="smoke",alias="thermometer",alias="valve",alias="voc")]
    pub data: T,
}

impl<T> SensorRecord<T> where T: RecordDataBounds {
    fn capability(&self)->SensorCapability {
        T::capability()
    }

    fn generic_description(&self)->String {
        format!("(id:\"{}\",device_id:\"{}\",sensor_no:{},time_recorded:{},{})", 
            self.id, self.device_id, self.sensor_no, self.time_recorded, self.capability().property_name())
    }

    fn description(&self)->String {
        match ron::to_string(self) {
            Ok(s) => s,
            Err(e) => self.generic_description()
        }
    }
}

// we would like a more expressive filename but the Delphire record database uses a random key
// so we generate a filename from the record info using the scheme
//    YYYYMMDD-HHmmSS_SSS-<device_id>-<sensor_no>-<image_record_id>.webp
impl SensorRecord<ImageData> {
    pub fn odin_filename (&self)->String {
        let ext = get_filename_extension(&self.data.filename).unwrap_or("");

        format!("{}-{}-{}-{}.{}",
            self.time_recorded.format("%Y%m%d-%H%M%S_%3f"),
            self.device_id,
            self.sensor_no,
            self.id,
            ext
        )
    }

    pub fn set_local_filename (&mut self) {
        self.data.local_filename = Some(self.odin_filename());
    }
}

impl<T> Dated for SensorRecord<T> where T: RecordDataBounds {
    fn date (&self)->DateTime<Utc> {
        self.time_recorded
    }
}

impl<T> Serialize for SensorRecord<T> where T: RecordDataBounds {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> where S: Serializer {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("SensorRecord", 7)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("timeRecorded", &to_epoch_millis(self.time_recorded))?; // our JS module expects epoch millis
        state.serialize_field("sensorNo", &self.sensor_no)?;
        state.serialize_field("deviceId", &self.device_id)?;
        state.serialize_field("evidences", &self.evidences)?;
        state.serialize_field("claims", &self.claims)?;
        state.serialize_field( T::capability().property_name(), &self.data)?; // map generic 'data' back into original property name
        state.end()
    }
}

impl<T> Ord for SensorRecord<T> where T: RecordDataBounds {
    fn cmp(&self, other: &Self) -> Ordering {
        self.time_recorded.cmp(&other.time_recorded)
    }
}

impl<T> PartialOrd for SensorRecord<T> where T: RecordDataBounds {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.time_recorded.cmp(&other.time_recorded))
    }
}

impl<T> PartialEq for SensorRecord<T> where T: RecordDataBounds {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl<T> Eq for SensorRecord<T> where T: RecordDataBounds {}

#[derive(Serialize,Deserialize,Debug,PartialEq,Clone)]
pub struct RecordRef {
    pub id: RecordId,
}

/// enum to give us a single non-generic type we can use to wrap any record so that we can publish it through a single msg/callback slot.
/// since the variants are just Arcs a SentinelUpdate is cheap to clone
/// note this also defines respective From<SensorRecord<..>> impls
define_algebraic_type!{ 
    #[serde(untagged)]
    pub SentinelUpdate: Clone + Serialize =
        Arc<SensorRecord<AccelerometerData>> |
        Arc<SensorRecord<AnemometerData>> |
        Arc<SensorRecord<CloudcoverData>> |
        Arc<SensorRecord<EventData>> |
        Arc<SensorRecord<FireData>> |
        Arc<SensorRecord<GasData>> |
        Arc<SensorRecord<GpsData>> |
        Arc<SensorRecord<GyroscopeData>> |
        Arc<SensorRecord<ImageData>> |
        Arc<SensorRecord<MagnetometerData>> |
        Arc<SensorRecord<OrientationData>> |
        Arc<SensorRecord<PersonData>> |
        Arc<SensorRecord<PowerData>> |
        Arc<SensorRecord<SmokeData>> |
        Arc<SensorRecord<ThermometerData>> |
        Arc<SensorRecord<ValveData>> |
        Arc<SensorRecord<VocData>>

    pub fn record_id (&self)->&RecordId { &__.id }
    pub fn device_id (&self)->&DeviceId { &__.device_id }
    pub fn sensor_no (&self)->u32 { __.sensor_no }
    pub fn time_recorded (&self)->DateTime<Utc> { __.time_recorded }
    pub fn capability (&self)->SensorCapability { __.capability() }
    pub fn description (&self)->String { __.description() }

    pub fn to_json (&self)->Result<String> { Ok(serde_json::to_string(&__)?) }
    pub fn to_json_pretty (&self)->Result<String> { Ok(serde_json::to_string_pretty(&__)?) }
}

/* #endregion sensor record */

/* #region record payload data *********************************************************************************/

macro_rules! define_sensor_data {
    ( $capa:ident = $( $body:tt )* ) => {
        paste! {
            #[derive(Serialize,Deserialize,Debug,PartialEq,Clone)]
            #[serde(rename_all="camelCase")]
            pub struct [<$capa Data>] {
                $( $body )*
            }
            impl CapabilityProvider for [<$capa Data>] {
                fn capability()->SensorCapability { SensorCapability::$capa }
            }
        }
    }
}

define_sensor_data! { Accelerometer =
    pub ax: f32,
    pub ay: f32,
    pub az: f32,
}

define_sensor_data! { Anemometer = 
    pub angle: Angle,
    pub speed: Velocity 
}

define_sensor_data! { Cloudcover =
    pub percent: f32,
}

define_sensor_data! { Event =
    pub event_code: String,
    pub original_type: Option<String>, // can have null value
}

define_sensor_data! { Fire =
    pub fire_prob: f64
}

define_sensor_data! { Image =
    pub filename: String, // this is the remote filename from the Delphire server, which is just a random string
    pub is_infrared: bool,
    pub orientation_record: Option<RecordRef>, // nested orientation record?
    #[serde(skip_deserializing)] pub local_filename: Option<String>, // this is our local filename that shows date, device and sensor
}

define_sensor_data! { Gas =
    pub gas: i32, // long
    pub humidity: f64,
    pub pressure: f64,
    pub altitude: f64
}

define_sensor_data! { Gps =
    pub latitude: LatAngle, //f64,
    pub longitude: LonAngle,//f64
    pub altitude: Option<f64>, // update to uom
    pub quality: Option<f64>,
    pub number_of_satellites: Option<i32>,
    #[serde(alias = "HDOP")] pub hdop: Option<f32>
}

define_sensor_data! { Gyroscope =
    pub gx: f64,
    pub gy: f64,
    pub gz: f64
}

define_sensor_data! { Orientation =
    pub w: f64,
    pub qx: f64,
    pub qy: f64,
    pub qz: f64
}

define_sensor_data! { Magnetometer =
    pub mx: f64,
    pub my: f64,
    pub mz: f64
}

define_sensor_data! { Person =
    pub person_prob: f64
}

define_sensor_data! { Power = // can use uom here for current, volatage, temp?
    pub battery_voltage: ElectricPotential,
    pub battery_current: ElectricCurrent,
    pub solar_voltage:ElectricPotential,
    pub solar_current: ElectricCurrent,
    pub load_voltage: ElectricPotential,
    pub load_current: ElectricCurrent,
    pub soc: f64,
    pub battery_temp: ThermodynamicTemperature, // temp
    pub controller_temp: ThermodynamicTemperature, //temp
}

define_sensor_data! { Smoke =
    pub smoke_prob: f64
}

define_sensor_data! { Thermometer =
    pub temperature: ThermodynamicTemperature
}

define_sensor_data! { Valve =
    pub valve_open: bool,
    pub external_light_on: bool,
    pub internal_light_on: bool,
}

define_sensor_data! { Voc =
   #[serde(rename = "TVOC")] pub tvoc: i32,
   #[serde(rename = "eCO2")] pub e_co2: i32,
}

#[derive(Serialize,Deserialize,Debug,PartialEq,Copy,Clone,IntoStaticStr)] 
#[serde(rename_all="lowercase")]
#[strum(serialize_all="lowercase")]
pub enum SensorCapability {
    Accelerometer,
    Anemometer,
    Cloudcover,
    Event,
    Fire,
    Gas,
    Gps,
    Gyroscope,
    Image,
    Magnetometer,
    Orientation,
    Person,
    Power,
    Smoke,
    Thermometer,
    Valve,
    Voc
}

impl SensorCapability {
    fn property_name (&self)->&'static str { self.into() }

    fn capability_of (rec_type: &str)->Option<SensorCapability> {
        match rec_type {
            "accelerometer" => Some( Self::Accelerometer ),
            "anemometer"    => Some( Self::Anemometer ),
            "cloudcover"    => Some( Self::Cloudcover ),
            "event"         => Some( Self::Event ),
            "fire"          => Some( Self::Fire ),
            "gas"           => Some( Self::Gas ),
            "gps"           => Some( Self::Gps ),
            "gyroscope"     => Some( Self::Gyroscope ),
            "image"         => Some( Self::Image ),
            "magnetometer"  => Some( Self::Magnetometer ),
            "orientation"   => Some( Self::Orientation ),
            "person"        => Some( Self::Person ),
            "power"         => Some( Self::Power ),
            "smoke"         => Some( Self::Smoke ),
            "thermometer"   => Some( Self::Thermometer ),
            "valve"         => Some( Self::Valve ),
            "voc"           => Some( Self::Voc ),
            _               => None
        }
    }
}

/* #endregion record payload data */

/* #region other query responses **********************************************************************/

#[derive(Serialize,Deserialize,Debug,PartialEq,Clone)]
pub struct Device {
    pub id: String,
    pub info: Option<String>
}

#[derive(Serialize,Deserialize,Debug,PartialEq,Clone)]
#[serde(rename_all="camelCase")]
pub struct DeviceList {
   pub data: Vec<Device>,
   // we assume we always query all devices i.e. don't need pagination
}
impl DeviceList {
    pub fn get_device_ids (&self)->Vec<String> {
        self.data.iter().map(|d| d.id.clone()).collect()
    }
    pub fn is_empty(&self)->bool { self.data.is_empty() }
}

#[derive(Serialize,Deserialize,Debug, PartialEq, Clone)]
#[serde(rename_all="camelCase")]
pub struct SensorData {
    pub no: u32,
    pub device_id: String,
    pub part_no: Option<String>,
    pub capabilities: Vec<SensorCapability>
}

#[derive(Serialize,Deserialize,Debug,PartialEq,Clone)]
#[serde(rename_all="camelCase")]
pub struct SensorList {
   pub data: Vec<SensorData>,
// we assume we always query all sensors i.e. don't need pagination
}

#[derive(Serialize,Deserialize,Debug,PartialEq,Clone)]
#[serde(bound = "T: RecordDataBounds")]
pub struct RecordList<T> where T: RecordDataBounds {
    pub data: Vec<SensorRecord<T>>,
}

/* #endregion other query responses */

/* #region internal data store ************************************************************************/

/// struct that stores SensorRecords and provides access to them either through respective device_id + sensor capability
/// or through their unique record_ids. The number of records per device+capability is bounded. All mutators have to
/// ensure consistency between the different access path fields
#[derive(Debug)]
pub struct SentinelStore {
    sentinels: HashMap<DeviceId,Sentinel>,
    updates: HashMap<RecordId,SentinelUpdate>
}
impl SentinelStore {
    pub fn new ()->Self {
        SentinelStore { sentinels: HashMap::new(), updates: HashMap::new() }
    }
    
    pub async fn fetch_from_config (&mut self, client: &Client, config: &SentinelConfig)->Result<()> {
        let base_uri = config.base_uri.as_str();
        let access_token = config.access_token.as_str();
        let n_last = config.max_history_len;  // number of initial records to retrieve
        let max_len = config.max_history_len; // max number of records to keep
        let device_filter = &config.device_filter;
    
        let device_list = get_device_list( client, base_uri, access_token).await?;

        for device in &device_list.data {
            if device_filter.is_empty() || device_filter.contains( &device.id) {
                let device_name = if let Some(info) = &device.info { info.clone() } else { "?".to_string() };
                let mut sentinel = Sentinel::new( device.id.clone(), device_name, max_len);
        
                let sensor_list = get_sensor_list( client, base_uri, access_token, device.id.as_str()).await?;
                for sensor_data in &sensor_list.data {
                    for capability in &sensor_data.capabilities {
                        let updates = sentinel.init_records(client, base_uri, access_token, sensor_data.no, *capability, n_last, max_len).await?;
                        for u in updates { self.updates.insert( u.record_id().clone(), u); }
                    }
                }
        
                sentinel.set_time_recorded(); // from latest sensor record
                self.insert( sentinel.device_id.clone(), sentinel);
            }
        }
    
        Ok(())
    }

    pub fn is_empty(&self)->bool {
        self.sentinels.is_empty()
    }

    pub fn insert (&mut self, k: String, v: Sentinel)->Option<Sentinel> {
        self.sentinels.insert( k, v)
    }

    pub fn get (&self, k: &String)->Option<&Sentinel> {
        self.sentinels.get(k)
    }

    pub fn get_mut (&mut self, k: &String)->Option<&mut Sentinel> {
        self.sentinels.get_mut(k)
    }

    pub fn sentinel_of (&mut self, k: &String)->Result<&mut Sentinel> {
        self.sentinels.get_mut( k).ok_or( OdinSentinelError::NoSuchDeviceError( k.to_string()))
    }

    pub fn values (&self)->Vec<&Sentinel> {
        self.sentinels.values().collect()
    }

    pub fn values_iter (&self)->impl Iterator<Item = &Sentinel> {
        self.sentinels.values()
    }

    pub fn get_device_ids (&self)->Vec<String> {
        self.sentinels.keys().map( |k| k.clone()).collect()
    }

    pub fn get_update (&self, k: &RecordId)->Option<&SentinelUpdate> {
        self.updates.get(k)
    }

    pub fn to_json (&self, pretty: bool)->Result<String> {
        let list = SentinelList { sentinels: self.values() };
        if pretty {
            Ok(serde_json::to_string_pretty( &list)?)
        } else {
            Ok(serde_json::to_string( &list)?)
        }
    }

    pub fn to_json_pretty (&self)->Result<String> {
        let list = SentinelList { sentinels: self.values() };
        Ok(serde_json::to_string_pretty( &list)?)
    }

    pub fn to_ron (&self, pretty: bool)->Result<String> {
        let list = SentinelList { sentinels: self.values() };
        if pretty {
            Ok(ron::ser::to_string_pretty( &list, ron::ser::PrettyConfig::default())?)
        } else {
            Ok(ron::to_string(&list)?)
        }
    }

    // here our responsibility is to keep sentinels and updates in sync and report back what changed
    pub fn update_with (&mut self, sentinel_update: SentinelUpdate, max_len: usize)->SentinelChange {
        let update = sentinel_update.clone(); // we have to do this prior to loosing ownership

        if let Some(ref mut sentinel) = self.sentinels.get_mut( sentinel_update.device_id()) {
            let (added_rec_id, removed_rec_id) = sentinel.update_with( sentinel_update);

            let added = if let Some(added_rec_id) = added_rec_id { 
                self.updates.insert(added_rec_id.clone(), update.clone());
                Some(update) 
            } else { 
                None  // nothing added
            };

            let removed = if let Some(removed_rec_id) = removed_rec_id { 
                self.updates.remove(&removed_rec_id) // only report as removed if it was stored
            } else { 
                None  // nothing removed
            };

            SentinelChange{ added, removed }

        } else { // add it as a new Sentinel (we could also reject here)
            let mut new_sentinel = Sentinel::new( sentinel_update.device_id().clone(), "?".to_string(), max_len);
            self.updates.insert( sentinel_update.record_id().clone(), sentinel_update.clone());

            new_sentinel.update_with( sentinel_update);
            self.sentinels.insert( new_sentinel.device_id.clone(), new_sentinel);

            SentinelChange{ added: Some(update), removed: None } // unknown device, nothing to do
        }
    }

    pub fn latest_records (&self)->HashMap<String,String> {
        let mut latest_recs: HashMap<String,String> = HashMap::new();
        for (_,sentinel) in &self.sentinels {
            sentinel.add_latest_records(&mut latest_recs);
        }
        latest_recs
    }
}

pub struct SentinelChange { added: Option<SentinelUpdate>, removed: Option<SentinelUpdate> }

/// helper type so that we can serialize the Sentinel values as a list
#[derive(Serialize)]
struct SentinelList<'a>  {
    sentinels: Vec<&'a Sentinel>
}

/// the current sentinel state. This needs to be serializable to JSON so that we
/// can send it to connected clients (field names have to map into what our javascript module expects)
define_struct! {
    #[serde(rename_all="camelCase")]
    pub Sentinel: Serialize + Deserialize + Debug =
        device_id: DeviceId,
        device_name: String,

        #[serde(skip_serializing_if = "Option::is_none", serialize_with = "odin_common::datetime::ser_epoch_millis_option")]
        time_recorded: Option<DateTime<Utc>> = None, // the latest record timestamp we have

        // the last N records for each capability/sensor
        accelerometer: VecDeque< Arc<SensorRecord<AccelerometerData>> > = VecDeque::new(),
        anemometer:    VecDeque< Arc<SensorRecord<AnemometerData>> > = VecDeque::new(),
        cloudcover:    VecDeque< Arc<SensorRecord<CloudcoverData>> > = VecDeque::new(),
        event:         VecDeque< Arc<SensorRecord<EventData>> > = VecDeque::new(),
        fire:          VecDeque< Arc<SensorRecord<FireData>> > = VecDeque::new(),
        gas:           VecDeque< Arc<SensorRecord<GasData>> > = VecDeque::new(),
        gps:           VecDeque< Arc<SensorRecord<GpsData>> > = VecDeque::new(),
        gyro:          VecDeque< Arc<SensorRecord<GyroscopeData>> > = VecDeque::new(),
        image:         VecDeque< Arc<SensorRecord<ImageData>> > = VecDeque::new(),
        mag:           VecDeque< Arc<SensorRecord<MagnetometerData>> > = VecDeque::new(),
        orientation:   VecDeque< Arc<SensorRecord<OrientationData>> > = VecDeque::new(),
        person:        VecDeque< Arc<SensorRecord<PersonData>> > = VecDeque::new(),
        power:         VecDeque< Arc<SensorRecord<PowerData>> > = VecDeque::new(),
        smoke:         VecDeque< Arc<SensorRecord<SmokeData>> > = VecDeque::new(),
        thermometer:   VecDeque< Arc<SensorRecord<ThermometerData>> > = VecDeque::new(),
        valve:         VecDeque< Arc<SensorRecord<ValveData>> > = VecDeque::new(),
        voc:           VecDeque< Arc<SensorRecord<VocData>> > = VecDeque::new(),

        #[serde(skip)]
        updates: HashMap<String,SentinelUpdate> = HashMap::new(), // record_id -> SentinelUpdate

        #[serde(skip)]
        max_len: usize
}


impl Sentinel {
    /// initial bulk retrieval based on capability. Note these are homogenous record type retrievals, i.e. we know (and check) the type
    /// of the returned records
    pub async fn init_records( &mut self, client: &Client, base_uri: &str, access_token: &str, 
                               sensor_no: u32, capability: SensorCapability, n_last: usize, max_len: usize)->Result<Vec<SentinelUpdate>> {
        let device_id = &self.device_id.as_str();
        use SensorCapability::*;
        let updates = match capability {
            Accelerometer => init_recs( &mut self.accelerometer, get_time_sorted_records( client, base_uri, access_token, device_id, sensor_no, n_last).await?, max_len),
            Anemometer    => init_recs( &mut self.anemometer,    get_time_sorted_records( client, base_uri, access_token, device_id, sensor_no, n_last).await?, max_len),
            Cloudcover    => init_recs( &mut self.cloudcover,    get_time_sorted_records( client, base_uri, access_token, device_id, sensor_no, n_last).await?, max_len),
            Event         => init_recs( &mut self.event,         get_time_sorted_records( client, base_uri, access_token, device_id, sensor_no, n_last).await?, max_len),
            Fire          => init_recs( &mut self.fire,          get_time_sorted_records( client, base_uri, access_token, device_id, sensor_no, n_last).await?, max_len),
            Gas           => init_recs( &mut self.gas,           get_time_sorted_records( client, base_uri, access_token, device_id, sensor_no, n_last).await?, max_len),
            Gps           => init_recs( &mut self.gps,           get_time_sorted_records( client, base_uri, access_token, device_id, sensor_no, n_last).await?, max_len),
            Gyroscope     => init_recs( &mut self.gyro,          get_time_sorted_records( client, base_uri, access_token, device_id, sensor_no, n_last).await?, max_len),
            Image         => init_image_recs( &mut self.image,   get_time_sorted_records( client, base_uri, access_token, device_id, sensor_no, n_last).await?, max_len),
            Magnetometer  => init_recs( &mut self.mag,           get_time_sorted_records( client, base_uri, access_token, device_id, sensor_no, n_last).await?, max_len),
            Orientation   => init_recs( &mut self.orientation,   get_time_sorted_records( client, base_uri, access_token, device_id, sensor_no, n_last).await?, max_len),
            Person        => init_recs( &mut self.person,        get_time_sorted_records( client, base_uri, access_token, device_id, sensor_no, n_last).await?, max_len),
            Power         => init_recs( &mut self.power,         get_time_sorted_records( client, base_uri, access_token, device_id, sensor_no, n_last).await?, max_len),
            Smoke         => init_recs( &mut self.smoke,         get_time_sorted_records( client, base_uri, access_token, device_id, sensor_no, n_last).await?, max_len),
            Thermometer   => init_recs( &mut self.thermometer,   get_time_sorted_records( client, base_uri, access_token, device_id, sensor_no, n_last).await?, max_len),
            Valve         => init_recs( &mut self.valve,         get_time_sorted_records( client, base_uri, access_token, device_id, sensor_no, n_last).await?, max_len),
            Voc           => init_recs( &mut self.voc,           get_time_sorted_records( client, base_uri, access_token, device_id, sensor_no, n_last).await?, max_len),
        };
        Ok(updates)
    }

    pub fn update_with( &mut self, sentinel_update: SentinelUpdate)->(Option<RecordId>,Option<RecordId>) {
        match_algebraic_type! { sentinel_update: SentinelUpdate as
            Arc<SensorRecord<AccelerometerData>> => sort_in_record( &mut self.accelerometer, sentinel_update, self.max_len),
            Arc<SensorRecord<AnemometerData>>    => sort_in_record( &mut self.anemometer,    sentinel_update, self.max_len),
            Arc<SensorRecord<CloudcoverData>>    => sort_in_record( &mut self.cloudcover,    sentinel_update, self.max_len),
            Arc<SensorRecord<EventData>>         => sort_in_record( &mut self.event,         sentinel_update, self.max_len),
            Arc<SensorRecord<FireData>>          => sort_in_record( &mut self.fire,          sentinel_update, self.max_len),
            Arc<SensorRecord<GasData>>           => sort_in_record( &mut self.gas,           sentinel_update, self.max_len),
            Arc<SensorRecord<GpsData>>           => sort_in_record( &mut self.gps,           sentinel_update, self.max_len),
            Arc<SensorRecord<GyroscopeData>>     => sort_in_record( &mut self.gyro,          sentinel_update, self.max_len),
            Arc<SensorRecord<ImageData>>         => sort_in_record( &mut self.image,         sentinel_update, self.max_len),
            Arc<SensorRecord<MagnetometerData>>  => sort_in_record( &mut self.mag,           sentinel_update, self.max_len),
            Arc<SensorRecord<OrientationData>>   => sort_in_record( &mut self.orientation,   sentinel_update, self.max_len),
            Arc<SensorRecord<PersonData>>        => sort_in_record( &mut self.person,        sentinel_update, self.max_len),
            Arc<SensorRecord<PowerData>>         => sort_in_record( &mut self.power,         sentinel_update, self.max_len),
            Arc<SensorRecord<SmokeData>>         => sort_in_record( &mut self.smoke,         sentinel_update, self.max_len),
            Arc<SensorRecord<ThermometerData>>   => sort_in_record( &mut self.thermometer,   sentinel_update, self.max_len),
            Arc<SensorRecord<ValveData>>         => sort_in_record( &mut self.valve,         sentinel_update, self.max_len),
            Arc<SensorRecord<VocData>>           => sort_in_record( &mut self.voc,           sentinel_update, self.max_len)
        }
    }

    pub fn add_latest_records (&self, latest_recs: &mut HashMap<String,String>) {
        add_latest_recs( &self.accelerometer, latest_recs);
        add_latest_recs( &self.anemometer, latest_recs);
        add_latest_recs( &self.cloudcover, latest_recs);
        add_latest_recs( &self.event, latest_recs);
        add_latest_recs( &self.fire, latest_recs);
        add_latest_recs( &self.gas, latest_recs);
        add_latest_recs( &self.gps, latest_recs);
        add_latest_recs( &self.gyro, latest_recs);
        add_latest_recs( &self.image, latest_recs);
        add_latest_recs( &self.mag, latest_recs);
        add_latest_recs( &self.orientation, latest_recs);
        add_latest_recs( &self.person, latest_recs);
        add_latest_recs( &self.power, latest_recs);
        add_latest_recs( &self.smoke, latest_recs);
        add_latest_recs( &self.thermometer, latest_recs);
        add_latest_recs( &self.valve, latest_recs);
        add_latest_recs( &self.voc, latest_recs);
    }
 
    pub fn set_time_recorded (&mut self) {
        fn set_latest<T> (latest: &mut DateTime<Utc>, recs: &VecDeque<Arc<SensorRecord<T>>>) where T: RecordDataBounds {
            if let Some(rec_date) = recs.front().map(|r| r.time_recorded) { 
                if rec_date > *latest { *latest = rec_date }
            } 
        }

        let mut latest = DateTime::from_timestamp_millis(0).unwrap();
        set_latest(&mut latest, &self.accelerometer);
        set_latest(&mut latest, &self.anemometer);
        set_latest(&mut latest, &self.cloudcover);
        set_latest(&mut latest, &self.event);
        set_latest(&mut latest, &self.fire);
        set_latest(&mut latest, &self.gas);
        set_latest(&mut latest, &self.gps);
        set_latest(&mut latest, &self.gyro);
        set_latest(&mut latest, &self.image);
        set_latest(&mut latest, &self.mag);
        set_latest(&mut latest, &self.orientation);
        set_latest(&mut latest, &self.person);
        set_latest(&mut latest, &self.power);
        set_latest(&mut latest, &self.smoke);
        set_latest(&mut latest, &self.thermometer);
        set_latest(&mut latest, &self.valve);
        set_latest(&mut latest, &self.voc);

        if latest.timestamp_millis() > 0 {
            self.time_recorded = Some(latest)
        }
    }

    pub fn get_position_at (&self, dt: DateTime<Utc>)->Option<DatedGeoPos> {
        if let Some(i_gps) = get_closest_record_idx( dt, &self.gps) {
            let gps = &self.gps[i_gps].data;
            if let Some(i_gas) = get_closest_record_idx( dt, &self.gas) {
                let gas = &self.gas[i_gas].data;
                Some( DatedGeoPos::new( gps.latitude, gps.longitude, gas.altitude, dt) )
            } else {
                Some( DatedGeoPos::new( gps.latitude, gps.longitude, gps.altitude.unwrap_or(0.0), dt) )
            }
        } else {
            None
        }
    }


}

pub fn get_closest_record_idx<T> (dt: DateTime<Utc>, recs: &VecDeque< Arc<SensorRecord<T>> >)->Option<usize> 
    where T: RecordDataBounds
{
    if recs.is_empty() {
        None

    } else {
        let millis = dt.timestamp_millis();
        let mut d = u64::MAX;
        let mut idx = 0;

        for i in 0..recs.len() {
            let nd = i64::abs_diff(recs[i].time_recorded.timestamp_millis(), millis);
            if nd == 0 { return Some(i) }

            if nd > d { 
                return Some(idx) 
            } else {
                idx = i;
                d = nd;
            }
        }
        Some(idx)
    }
}

pub fn rec_key (device_id: &str, sensor_no: u32, capa: SensorCapability)->String {
    format!("/devices/{}/sensors/{}/{}", device_id, sensor_no, capa.property_name())
}

fn add_latest_recs<T> (list: &VecDeque<Arc<SensorRecord<T>>>, latest_recs: &mut HashMap<String,String>)     
    where T: RecordDataBounds
{
    let mut sensors: Vec<u32> = Vec::with_capacity(2);
    for rec in list {
        let sensor_no = rec.sensor_no;
        if !sensors.contains( &sensor_no) {
            let key = rec_key( &rec.device_id, sensor_no, rec.capability());
            latest_recs.insert( key, rec.id.clone());
            sensors.push( sensor_no);
        }
    }
}

// note that most record lists only have a single sensor, but some (images) have several. The max_len is per
// sensor so we have to sort in recs after the first sensor batch
fn init_recs<T> (list: &mut VecDeque<Arc<SensorRecord<T>>>, recs: Vec<SensorRecord<T>>, max_len: usize)->Vec<SentinelUpdate> 
    where T: RecordDataBounds, SentinelUpdate: From<Arc<SensorRecord<T>>>
{
    let mut updates = Vec::<SentinelUpdate>::with_capacity(recs.len());

    if list.is_empty() { // first sensor, recs is already time sorted
        for rec in recs.into_iter() {
            let rec = Arc::new(rec);
            updates.push( rec.clone().into()); 
            list.push_back( rec);
        }
    } else { // different sensor, we have to sort in
        for rec in recs.into_iter() {
            let rec = Arc::new(rec);
            updates.push( rec.clone().into()); 
            sort_in_record( list, rec, max_len);
        }
    }

    updates
}

// this is a pain - we need to set the local filename explicitly
fn init_image_recs (list: &mut VecDeque<Arc<SensorRecord<ImageData>>>, mut recs: Vec<SensorRecord<ImageData>>, max_len: usize)->Vec<SentinelUpdate> {
    for rec in recs.iter_mut() {
        rec.set_local_filename()
    }
    init_recs(list, recs, max_len)
}

/// sort in record according to timestamp (newer records first). Note this transfers ownership of 'rec'.
/// owner-specific housekeeping can be performed through provided (optional) closures
pub fn sort_in_record<T> (list: &mut VecDeque<Arc<SensorRecord<T>>>, rec: Arc<SensorRecord<T>>, max_len: usize)->(Option<RecordId>,Option<RecordId>)
    where T: RecordDataBounds, SentinelUpdate: From<Arc<SensorRecord<T>>>
{
    let mut n_sensor_recs = 0;
    let mut added: Option<RecordId> = None;
    let mut removed: Option<RecordId> = None;
    let sensor_no = rec.sensor_no;

    for (i,r) in list.iter().enumerate() {
        if rec.time_recorded > r.time_recorded { // insert record
            added = Some(rec.id.clone());
            list.insert( i, rec);
            removed = remove_excess_sensor_rec( list, sensor_no, i+1, n_sensor_recs, max_len);

            return (added,removed)

        } else if rec.time_recorded == r.time_recorded {
            if rec.id == r.id && sensor_no == r.sensor_no { // replace record, no need to add or remove
                list[i] = rec;
                return (None,None)
            }
        }
        if sensor_no == r.sensor_no { n_sensor_recs += 1; } 
    }

    // if we get here it's the oldest record so check if it would exceed max_len before adding
    if n_sensor_recs < max_len {
        added = Some(rec.id.clone());
        list.push_back( rec);
    }

    (added,removed)
}

// find the first sensor rec from start_idx that exceeds max_len
fn remove_excess_sensor_rec<T> (list: &mut VecDeque<Arc<SensorRecord<T>>>, sensor_no: u32, 
                                start_idx: usize, n_sensor_recs: usize, max_len: usize)->Option<RecordId> 
    where T: RecordDataBounds
{
    let mut n = n_sensor_recs;

    for i in start_idx..list.len() {
        if list[i].sensor_no == sensor_no { n += 1 }
        if n > max_len { 
            return list.remove(i).map( |r| r.id.clone() );
        }
    }

    None
}

/* #endregion internal data store */

/* #region config  ************************************************************************************/

#[derive(Deserialize,Serialize,Debug)]
#[serde(default)]
pub struct SentinelConfig {
    pub base_uri: String,
    pub ws_uri: String,
    pub(crate) access_token: String, // TODO - should probably be a [u8;N]

    pub max_history_len: usize, // maximum number of records to store per device/sensor capability
    pub max_age: Duration, // maximum age after which additional data (images etc.) are deleted
    pub ping_interval: Option<Duration>, // interval duration for sending Ping messages on the websocket 
    pub reconnect_delay: Option<Duration>, // sleep duration after which we try to re-initializa a broken websocket 
    pub device_filter: Vec<String>, // optional list of device_ids to filter for

    pub inactive_duration: Duration, // max duration since last update after which a device is considered to be inactive
    pub inactive_interval: Duration  // how often we check for inactive devices
}

impl Default for SentinelConfig {
    fn default()->Self {
        SentinelConfig {
            //--- the ones that need to be set
            base_uri: "?".to_string(),
            ws_uri: "?".to_string(),
            access_token: "?".to_string(),

            //--- the fields for which we have defaults
            max_history_len: 10,
            max_age: Duration::from_secs( 60*60*24),
            ping_interval: Some(Duration::from_secs(25)),
            reconnect_delay: None,
            device_filter: Vec::new(), // default is no filter
            inactive_duration: Duration::from_secs( 7200), // inactive if no update for 2h
            inactive_interval: Duration::from_secs(300) // check every 5 min
        }
    }
}

pub fn sentinel_cache_dir()->PathBuf {
    let path = odin_build::cache_dir().join("sentinel");
    // Ok to panic - this is called during sys init
    ensure_writable_dir(&path).expect( &format!("invalid sentinel cache dir: {path:?}"));
    path
}


/// device information that is not obtained through Delphire server APIs 
#[derive(Serialize,Deserialize,Debug)]
#[serde(rename_all(serialize="camelCase"))]
pub struct SentinelDeviceInfo {
    pub name: String,
    pub sensor_infos: Vec<SensorInfo>,
    pub external_images: Vec<ExternalImage>,
}

impl SentinelDeviceInfo {
    pub fn to_json (&self)->Result<String> { Ok(serde_json::to_string(&self)?) }
    pub fn to_json_pretty (&self)->Result<String> { Ok(serde_json::to_string_pretty(&self)?) }
}

/// the type we use to configure additional SentinelDeviceInfos
pub type SentinelDeviceInfos = HashMap<String,SentinelDeviceInfo>;

#[derive(Serialize, Deserialize,Debug)]
#[serde(rename_all(serialize="camelCase"))]
pub struct SensorInfo {
    pub sensor_no: u32,
    pub smoke_prob: f32,
    pub fire_prob: f32,
    pub fov_left: Angle,
    pub fov_right: Angle,
    pub fov_dist: f64,
}

define_algebraic_type! { 
    pub ExternalImage: Serialize + Deserialize = DirectUriImage

    pub fn name (&self)->&str { __.name.as_str() }

    pub fn uri (&self)->&str { match self {
        Self::DirectUriImage(o) => o.uri.as_str()
    }}

    pub fn filename (&self)->&str { match self {
        Self::DirectUriImage(o) => o.filename.as_str()
    }}

    pub fn supports_sensor (&self, img_sensor_no: u32)->bool {
        __.sensors.is_empty() || __.sensors.contains(&img_sensor_no)
    }
}


/// something we can retrieve from a fixed URI with a simple GET
#[derive(Serialize,Deserialize,Debug)]
#[serde(rename="image")] // FIXME - this does not rename variant names ?
pub struct DirectUriImage {
    pub name: String,
    pub sensors: Vec<u32>, // Sentinel image sensor_no for which this external image can be used (empty means any one)
    pub filename: String, // this is the filename stem 
    pub uri: String,
    //... and probably more to follow (e.g. location, fov, optional classifier, ...)
}

/* #endregion config */


/* #region file requests ******************************************************************************************************/

/// a struct that associates a SensorRecord with a file (pathname)
#[derive(Debug,Clone,PartialEq)]
pub struct SentinelFile {
    pub record_id: String,   // the SensorRecord this file is associated with
    pub pathname: PathBuf,   // this is the physical location of the file (once downloaded)
}

/// message to request a SentinelFile. The fields are from the SensorRecord that contains the file reference
#[derive(Debug)]
pub struct GetSentinelFile {
    pub record_id: String,
    pub filename: String, // on the Delphire server. This is only used to construct the uri and not neccessarily how we store it locally
    pub uri: Option<String>, // in case this is an external file request
}
impl GetSentinelFile {
    pub fn internal(record_id: String, filename: String)->Self { GetSentinelFile {record_id, filename, uri: None } }
    pub fn external(record_id: String, filename: String, uri: String)->Self { GetSentinelFile {record_id, filename, uri: Some(uri) } }
}

pub type SentinelFileResult = Result<SentinelFile>;
pub type SentinelFileQuery = Query<GetSentinelFile,SentinelFileResult>;


/// this is a request for related imagery from external sources
/// note that record_id is NOT referring to an image record but to the event that caused this request
#[derive(Debug)]
pub struct GetExternalFile {
    pub record_id: String, // the record we associate this file with
    pub url: String,
    pub filename: String,
}

pub type ExternalFileQuery = Query<GetExternalFile,SentinelFileResult>;


/* #endregion file requests */

/* #region connectors  ********************************************************************************************************/

/// this is the abstraction over the actual source of external information, which can be either a live connection to a Delphire server
/// or a replayer for archived data
#[async_trait]
pub trait SentinelConnector {

    async fn start (&mut self, hself: ActorHandle<SentinelActorMsg>) -> Result<()>;
 
    // note that these future do not wait until the request was resolved - only until the request has been queued (if it needs to)

    async fn send_cmd (&mut self, cmd: ws::WsCmd) -> Result<()>;

    async fn handle_sentinel_file_query (&self, file_query: Query<GetSentinelFile,Result<SentinelFile>>) -> Result<()>;
 
    fn terminate (&mut self);
 
    fn max_history(&self)->usize;

    /// duration since last update after which we consider a device inactive
    fn inactive_duration(&self)->Duration;

    /// duration how often we check for inactive status
    fn inactive_interval(&self)->Duration;
 }

/* #endregion connectors */

/* #region basic http getters *************************************************************************************************/

// the reqwest::Response::json() alternative does not preserve enough error information
async fn from_json<T> (response: Response)->Result<T> where T: DeserializeOwned {
    let bytes = response.bytes().await?;
    serde_json::from_slice( &bytes).map_err(|e| {
        //let mut s = String::new();
        //bytes.reader().read_to_string(&mut s);
        //println!("{s}");

        OdinSentinelError::JsonError(e.to_string())
    })
}

pub async fn get_device_list (client: &Client, base_uri: &str, access_token: &str)->Result<DeviceList> {
    let uri = format!("{base_uri}/devices");
    let response = client.get(uri).bearer_auth(access_token).send().await?;
    let device_list: DeviceList = from_json(response).await?;
    Ok(device_list)
}

pub async fn get_device_list_from_config (client: &Client, config: &SentinelConfig)->Result<DeviceList> {
    get_device_list( client, &config.base_uri, &config.access_token).await
}

pub async fn get_sensor_list (client: &Client, base_uri: &str, access_token: &str, device_id: &str) -> Result<SensorList> {
    let uri =  format!("{base_uri}/devices/{device_id}/sensors");
    let response = client.get(uri).bearer_auth(access_token).send().await?;
    let sensor_list: SensorList = from_json(response).await?;
    Ok(sensor_list)
}

pub async fn get_time_sorted_records <T> (client: &Client, base_uri: &str, access_token: &str, 
                              device_id: &str, sensor_no:u32, n_last: usize) -> Result<Vec<SensorRecord<T>>> 
    where T: RecordDataBounds
{ 
    let capability = T::capability();
    let uri = format!("{base_uri}/devices/{device_id}/sensors/{sensor_no}/{capability:?}?sort=timeRecorded,DESC&limit={n_last}");
    let response = client.get(uri).bearer_auth(access_token).send().await?;
    let record_list: RecordList<T> = from_json(response).await?; 
    Ok(record_list.data)
} 

pub async fn get_records_since <T> (client: &Client, base_uri: &str, access_token: &str, uri_path: &str, last: &str) -> Result<Vec<SensorRecord<T>>> 
    where T: RecordDataBounds
{
    let uri = format!("{base_uri}/{uri_path}?sort=timeRecorded,DESC&last={last}");
    let response = client.get(uri).bearer_auth(access_token).send().await?;
    let record_list: RecordList<T> = from_json(response).await?; 
    Ok(record_list.data)
}

pub async fn get_latest_record <T> (client: &Client, base_uri: &str, access_token: &str, 
                                    device_id: &str, sensor_no:u32) -> Result<SensorRecord<T>> 
    where T: RecordDataBounds, SentinelUpdate: From<Arc<SensorRecord<T>>>
{
    let mut recs = get_time_sorted_records::<T>( client, base_uri, access_token, device_id, sensor_no, 1).await?;
    recs.pop().ok_or( no_data(format!("for device: {}, sensor: {}, capability: {:?}", device_id, sensor_no, T::capability())))
}

async fn get_file_request (client: &Client, access_token: &str, uri: &str, pathname: &PathBuf)->Result<()> {
    let mut response = client.get(uri).bearer_auth(access_token).send().await?;

    let mut file = File::create(pathname)?;
    while let Some(chunk) = response.chunk().await? {
        file.write(&chunk)?;
    }

    Ok(())
}

pub fn get_image_uri (base_uri: &str, record_id: &str)->String {
    format!("{base_uri}/images/{record_id}")
}

/* #endregion basic http getters */