/*
 * Copyright © 2025, United States Government, as represented by the Administrator of 
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

use std::{any::Any, collections::VecDeque, time::Duration, fmt};
use intmap::IntMap;
use serde::{de::DeserializeOwned, Deserialize, Serialize, Serializer};
use serde_json;
use chrono::{DateTime,Utc};
use http::header::ACCEPT;
use reqwest::{Client,Response,header::{HeaderMap,HeaderName,HeaderValue,CONTENT_TYPE}};
use ron::{self, to_string};
use async_trait::async_trait;
use uom::si::f64::{Angle, ElectricCurrent, ElectricPotential, Length, Pressure, ThermodynamicTemperature, Velocity};
use uom::si::{
    thermodynamic_temperature::{degree_celsius,degree_fahrenheit}, 
    pressure::{pascal,inch_of_mercury}, 
    velocity::{mile_per_hour,meter_per_second}
};
use odin_build::{define_load_asset, define_load_config, pkg_cache_dir};
use odin_server::{spa::SpaService,ws_service::ws_msg_from_json};
use odin_common::{
    collections::RingDeque, datetime::{self, EpochMillis, utc_now}, geo::GeoPoint, 
    json_writer::{JsonWritable,JsonWriter, NumFormat}, net::from_json, Percent,
    angle::Angle360,
};
use odin_actor::ActorHandle;

pub mod errors;
use errors::{Result,OdinN5Error};

pub mod actor;
pub use actor::*;

pub mod live_connector;
pub use live_connector::*;

pub mod n5_service;
use n5_service::N5Service;

/// crate to import N5 sensor data

define_load_config!{}
define_load_asset!{}

/* #region import types  **********************************************************************************/

// note these types are only used to deserialize imported data. Internally we work with physical units and aggregates

#[derive(Deserialize, Debug)]
pub struct DevicesResponse {
    results: Vec<Device>
}

#[derive(Deserialize, Debug)]
pub struct Device {
    pub id: u32,
    pub station_id: String,
    pub device_type: String,
    pub latest_status: Status,
}

#[derive(Deserialize, Debug)]
pub struct Status {
    pub online: bool,
    pub active: bool,
    pub activation_date: DateTime<Utc>,
    pub location: Location,
    pub location_description: String,
}

#[derive(Deserialize, Debug)]
pub struct Location {
    #[serde(rename = "static")]
    pub static_loc: Loc
}

#[derive(Deserialize,Debug)]
pub struct Loc {
    pub longitude: f64,
    pub latitude: f64,
}

#[derive(Deserialize, Debug)]
pub struct DataResponse {
    results: Vec<Data>
}

#[derive(Deserialize, Debug)]
pub struct Data {
    pub create_date: DateTime<Utc>,
    pub battery_soc: f64,
    pub temperature: f64,
    pub humidity: f64,
    pub pressure: f64,
    pub air_quality: f64,
}

#[derive(Deserialize, Debug)]
pub struct HeatMapResponse {
    results: Vec<HeatMap>
}

#[derive(Deserialize, Debug)]
pub struct HeatMap {
    pub create_date: DateTime<Utc>,
    pub ir_reading: Vec<i32>,
    pub ic_score: f64 
}

#[derive(Deserialize, Debug)]
pub struct SmokeIndexResponse {
    results: Vec<SmokeIndex>
}

#[derive(Deserialize, Debug)]
pub struct SmokeIndex {
    pub create_date: DateTime<Utc>,
    pub smoke_index_reading: u32
}

#[derive(Deserialize, Debug)]
pub struct WindResponse {
    results: Vec<Wind>
}

#[derive(Deserialize, Debug)]
pub struct Wind {
    pub create_date: DateTime<Utc>,
    wind_speed_reading: f64,
    wind_direction_reading: f64,
    source: String,
}

#[derive(Deserialize, Debug)]
pub struct AlertResponse {
    results: Vec<Alert>
}

#[derive(Deserialize, Debug)]
pub struct Alert {
    create_date: DateTime<Utc>,
    #[serde(alias="type")]
    alert_type: AlertType,
}

#[repr(u8)]
#[derive(Debug,Serialize,Deserialize,Clone,PartialEq)]
pub enum AlertType {
    FireAlert = 1,
    FireWarning = 2,
    AirQuality = 3,
    IrCamera = 50,
    GasDiscrepancy = 51,
    ParticleDiscrepancy = 52,
    SystemTest1 = 100,
    SystemTest2 = 101,
    SystemTest3 = 102,
}

/* #endregion import types */

/* #region internal data model *************************************************************************/

// we separate the import data types from our internal model so that we (a) can add units of measure
// and (b) have more control over deserialization (serde) and serialization (JsonWriter). The latter one
// is just used by odon_n5.js and should pre-process values

#[derive(Debug)]
pub struct N5Device {
    pub id: u32,
    pub position: GeoPoint,

    pub name: String,
    pub device_type: String,

    pub online: bool,
    pub active: bool,

    pub data: VecDeque<N5Data>,
    pub alerts: VecDeque<N5Alert>
}

impl N5Device {
    pub fn from(device: Device, config: &N5Config)->Self {
        let status = &device.latest_status;

        N5Device { 
            id:device.id, 
            position: GeoPoint::from_lon_lat_degrees( status.location.static_loc.longitude, status.location.static_loc.latitude), 
            name: device.station_id, 
            device_type: device.device_type, 
            online: status.online, 
            active: status.active, 

            data: VecDeque::with_capacity( config.max_history_len), 
            alerts: VecDeque::with_capacity( config.max_history_len), 
        }
    }

    pub fn add_data (&mut self, n5_data: N5Data) {
        self.data.push_to_ringbuffer(n5_data);
    }
}

impl JsonWritable for N5Device {
    fn write_json_to (&self, w: &mut JsonWriter) {
        w.write_object( |w| {
            w.write_field( "id", self.id);
            w.write_field( "name", &self.name);
           
            w.write_object_field( "position", |w|{
                w.write_f64_field("lon", self.position.longitude_degrees(), NumFormat::Fp5);
                w.write_f64_field("lat", self.position.latitude_degrees(), NumFormat::Fp5)
            });

            w.write_field( "device_type", &self.device_type);
            w.write_field( "online", self.online);
            w.write_field( "active", self.active);

            w.write_array_field("data", |w|{
                for d in &self.data { d.write_json_to(w);}
            });

            w.write_array_field("alerts", |w|{
                for a in &self.alerts { a.write_json_to(w);}
            });
        })
    }
}

/// the variable data of an N5Device for which we keep history
/// TODO - this could include positions should we at some point support mobile devices
#[derive(Debug,Clone)]
pub struct N5Data {
    pub date: EpochMillis,

    pub battery_soc: Percent,
    pub temperature: ThermodynamicTemperature,
    pub humidity: Percent,
    pub pressure: Pressure,

    pub wind_spd: Velocity,
    pub wind_dir: Angle360,

    pub ic_score: f64,   // IR reading
    pub smoke_index: f64,
    pub air_quality: f64, // PM 2.5 ? units? AQI ? 
}

impl JsonWritable for N5Data {
    fn write_json_to (&self, w: &mut JsonWriter) {
        w.write_object( |w| {
            w.write_field( "date", self.date.millis());
            w.write_field( "battery_soc", self.battery_soc.rounded_percent());
            w.write_field( "temperature", self.temperature.get::<degree_fahrenheit>() as i64);
            w.write_field( "humidity", self.humidity.rounded_percent());
            w.write_f64_field( "pressure", self.pressure.get::<inch_of_mercury>(), NumFormat::Fp2);
            w.write_f64_field( "wind_spd", self.wind_spd.get::<mile_per_hour>(), NumFormat::Fp1);
            w.write_f64_field( "wind_dir", self.wind_dir.degrees(), NumFormat::Fp0);
            w.write_f64_field( "ic_score", self.ic_score, NumFormat::Fp2);
            w.write_field( "smoke_index", self.smoke_index as i64); // units ?
            w.write_field( "air_quality", self.air_quality as i64); // units ?
        });
    }
}

#[derive(Serialize,Debug,PartialEq)]
pub struct N5Alert {
    pub date: EpochMillis,
    pub alert_type: AlertType
}

impl JsonWritable for N5Alert {
    fn write_json_to (&self, w: &mut JsonWriter) {
        w.write_object( |w| {
            w.write_field( "date", self.date.millis());
            w.write_field( "alert_type", self.alert_type.clone() as u8);
        })
    }
}

/// simple update aggregate
#[derive(Debug)]
pub struct N5DataUpdate {
    pub id: u32,
    pub data: N5Data
}

impl JsonWritable for N5DataUpdate {
    fn write_json_to (&self, w: &mut JsonWriter) {
        w.write_object( |w| {
            w.write_field( "id", self.id);
            w.write_json_field( "data", &self.data);
        })
    }
}

pub fn get_json_update_msg (updates: &[N5DataUpdate])->String {
    let mut w = JsonWriter::with_capacity(8192);
    w.write_object( |w| {
        w.write_field("date", datetime::utc_now().timestamp());
        w.write_array_field("changes", |w| {
            for d in updates {
                d.write_json_to(w);
            }
        })
    });
    ws_msg_from_json( N5Service::mod_path(), "update", w.as_str())
}


pub struct N5DeviceStore(IntMap<u32,N5Device>);

impl N5DeviceStore {
    pub fn new()->Self { 
        N5DeviceStore( IntMap::new()) 
    }

    pub fn add (&mut self, n5_device: N5Device) {
        self.0.insert( n5_device.id, n5_device);
    }

    pub fn add_all (&mut self, n5_devices: Vec<N5Device>) {
        for device in n5_devices {
            self.add( device)
        }
    }

    pub fn update_data (&mut self, updates: &[N5DataUpdate]) {
        for update in updates {
            if let Some(device) = self.0.get_mut(update.id) {
                device.add_data( update.data.clone());
            }
        }
    }

    pub fn write_json_snapshot_to (&self, w: &mut JsonWriter) {
        w.clear();

        w.write_object( |w| {
            w.write_field("date", datetime::utc_now().timestamp());
            w.write_array_field("devices", |w| {
                for device in self.0.values() {
                    device.write_json_to(w);
                }
            });
        });
    }

    /// this happens infrequently from a dyn action so we don't cache the writer (but for that save the clone)
    pub fn get_json_snapshot_msg (&self)->String {
        let mut w = JsonWriter::with_capacity(8192);
        self.write_json_snapshot_to(&mut w);
        ws_msg_from_json( N5Service::mod_path(), "snapshot", w.as_str())
    }
}


/* #region actor types **************************************************************************/


#[derive(Deserialize,Serialize,Debug)]
pub struct N5Config {
    pub base_uri: String,
    pub(crate) api_key: String,

    pub max_history_len: usize,
    pub data_cycles: usize,
    pub retrieve_interval: Duration,
    pub aggregate_interval: Duration,
}

#[async_trait]
pub trait N5Connector {
    async fn start (&mut self, hself: ActorHandle<N5ActorMsg>) -> Result<()>;
    //... more to follow
    fn terminate (&mut self);
}

/* #endregion actor types */

/* #region queries ********************************************************************************/

// low level (import data type) retrieval

pub async fn get_devices (client: &Client, conf: &N5Config)->Result<Vec<Device>> {
    let uri = format!("{}/devices?page_size=100&page=1&sort_dir=ASC", conf.base_uri);
    let response = get_response( client, conf, uri.as_str()).await?;
    let device_response: DevicesResponse = from_json( response).await?;
    Ok(device_response.results)
}

pub async fn get_data (client: &Client, conf: &N5Config, device_id: u32)->Result<Vec<Data>> {
    let uri = format!("{}/devices/{}/data?page_size={}&sort_dir=DESC", conf.base_uri, device_id, conf.data_cycles);
    let response = get_response( client, conf, uri.as_str()).await?;
    let data_response: DataResponse = from_json( response).await?;
    Ok(data_response.results)
}

pub async fn get_wind (client: &Client, conf: &N5Config, device_id: u32)->Result<Vec<Wind>> {
    // TODO - no average here?
    let uri = format!("{}/devices/{}/wind?page_size={}&sort_dir=DESC", conf.base_uri, device_id, conf.data_cycles);
    let response = get_response( client, conf, uri.as_str()).await?;
    let wind_response: WindResponse = from_json( response).await?;
    Ok(wind_response.results)
}

pub async fn get_heat_map (client: &Client, conf: &N5Config, device_id: u32)->Result<Vec<HeatMap>> {
    let avg_minutes = conf.aggregate_interval.as_secs() / 60;
    let uri = format!("{}/devices/{}/heat-map?page_size={}&sort_dir=DESC&interval={}-minutes", conf.base_uri, device_id, conf.data_cycles, avg_minutes);
    let response = get_response( client, conf, uri.as_str()).await?;
    let heat_map_response: HeatMapResponse = from_json( response).await?;
    Ok(heat_map_response.results)
}

pub async fn get_smoke_index (client: &Client, conf: &N5Config, device_id: u32)->Result<Vec<SmokeIndex>> {
    let avg_minutes = conf.aggregate_interval.as_secs() / 60;
    let uri = format!("{}/devices/{}/smoke-index?page_size={}&sort_dir=DESC&interval={}-minutes", conf.base_uri, device_id, conf.data_cycles, avg_minutes);
    let response = get_response( client, conf, uri.as_str()).await?;
    let smoke_index_response: SmokeIndexResponse = from_json( response).await?;
    Ok(smoke_index_response.results)
}

pub async fn get_alerts (client: &Client, conf: &N5Config, device_id: u32)->Result<Vec<Alert>> {
    let uri = format!("{}/devices/{}/alerts?page_size={}&sort_dir=DESC", conf.base_uri, device_id, conf.data_cycles);
    let response = get_response( client, conf, uri.as_str()).await?;
    let alert_response: AlertResponse = from_json( response).await?;
    Ok(alert_response.results)
}

async fn get_response (client: &Client, conf: &N5Config, uri: &str)->Result<Response> {
    let response = client.get(uri)
        .header( ACCEPT, HeaderValue::from_str("application/json")?)
        .header( "x-api-key", HeaderValue::from_str( conf.api_key.as_str())?)
        .send()
        .await?;

    Ok(response)
}

/// the high level data retrieval - this transforms 
pub async fn get_current_device_data (client: &Client, conf: &N5Config, device_id: u32)->Result<N5Data> {
    let data = get_data( client, conf, device_id).await?; // this gets the current snapshot
    let heat_map = get_heat_map( client, conf, device_id).await?;
    let wind = get_wind( client, conf, device_id).await?;
    //let smoke_index = get_smoke_index( client, conf, device_id).await?; // TODO activate when accessible

    if data.len() > 0 && heat_map.len() > 0 /*&& smoke_index.len() > 0 */ {
        let n5_data = N5Data {
            date: data[0].create_date.into(),
            battery_soc: Percent::new( data[0].battery_soc),
            temperature: ThermodynamicTemperature::new::<degree_celsius>( data[0].temperature),
            humidity: Percent::new( data[0].humidity),
            pressure: Pressure::new::<pascal>( data[0].pressure), // TODO - check unit
            wind_spd: Velocity::new::<meter_per_second>( wind[0].wind_speed_reading),
            wind_dir: Angle360::from_degrees( wind[0].wind_direction_reading),
            ic_score: heat_map[0].ic_score,   // IR reading
            smoke_index: 0.0, // smoke_index[0].smoke_index_reading // TODO activate when accessible
            air_quality: data[0].air_quality, // PM 2.5 ? units?
        };
        Ok(n5_data)
    } else {
        Err( OdinN5Error::OpFailedError("insufficient data".into()))
    }
}

/// initial N5Device retrieval
pub async fn get_n5_devices (client: &Client, conf: &N5Config, init: bool)->Result<Vec<N5Device>> {
    let devices = get_devices(client, conf).await?;
    let mut n5_devices: Vec<N5Device> = devices.into_iter().map( |d| N5Device::from(d, conf)).collect();

    if init {
        for d in n5_devices.iter_mut() {
            if d.active && d.online {
                if let Ok(n5_data) = get_current_device_data( client, conf, d.id).await {
                    d.add_data(n5_data);
                }
            }
        }
    }

    Ok(n5_devices)
}

pub async fn get_n5_data (client: &Client, conf: &N5Config, device_ids: &[u32])->Result<Vec<N5DataUpdate>> {
    let mut updates: Vec<N5DataUpdate> = Vec::with_capacity(device_ids.len());

    for id in device_ids {
        if let Ok(data) = get_current_device_data( client, conf, *id).await {
            updates.push( N5DataUpdate { id: *id, data });
        }
    }

    Ok(updates)
}

/* #endregion queries */
