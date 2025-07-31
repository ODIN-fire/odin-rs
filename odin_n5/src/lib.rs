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

use std::collections::{VecDeque,HashMap};
use serde::{de::DeserializeOwned, Deserialize, Serialize, Serializer};
use serde_json;
use chrono::{DateTime,Utc};
use http::header::ACCEPT;
use reqwest::{Client,Response,header::{HeaderMap,HeaderName,HeaderValue,CONTENT_TYPE}};
use ron::{self, to_string};
use async_trait::async_trait;
use odin_build::{define_load_asset, define_load_config, pkg_cache_dir};
use odin_common::{geo::GeoPoint,net::from_json};
use odin_actor::ActorHandle;

pub mod errors;
use errors::Result;

pub mod actor;
pub use actor::*;

pub mod live_connector;
pub use live_connector::*;

/// crate to import N5 sensor data

define_load_config!{}
define_load_asset!{}

/* #region types  **********************************************************************************/

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

    #[serde(skip_deserializing)]
    pub data: VecDeque<Data> // a ringbuffer with the last N data points
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
    pub ir_reading: Vec<u32>,
    pub ic_score: u32 
}

#[derive(Deserialize, Debug)]
pub struct AlertResponse {
    results: Vec<Alert>
}

#[derive(Deserialize, Debug)]
pub struct Alert {
    pub create_date: DateTime<Utc>,
    //... add alert details
}

/* #endregion types */

/* #region actor types **************************************************************************/

pub type DeviceStore = HashMap<u32,Device>;

#[derive(Debug)]
pub enum DeviceUpdate {
    Data(Data),
    HeatMap(HeatMap),
    Alert(Alert)
}

#[derive(Deserialize,Serialize,Debug)]
pub struct N5Config {
    pub base_uri: String,
    pub(crate) api_key: String,
}

#[async_trait]
pub trait N5Connector {
    async fn start (&mut self, hself: ActorHandle<N5ActorMsg>) -> Result<()>;
    //... more to follow
    fn terminate (&mut self);
}

/* #endregion actor types */

/* #region queries ********************************************************************************/

pub async fn get_devices (client: &Client, conf: &N5Config)->Result<Vec<Device>> {
    let uri = format!("{}/devices?page_size=100&page=1&sort_dir=ASC", conf.base_uri);
    let response = get_response( client, conf, uri.as_str()).await?;
    let device_response: DevicesResponse = from_json( response).await?;
    Ok(device_response.results)
}

pub async fn get_data (client: &Client, conf: &N5Config, device_id: u32, n_last: usize)->Result<Vec<Data>> {
    let uri = format!("{}/devices/{}/data?page_size={}&sort_dir=DESC", conf.base_uri, device_id, n_last);
    let response = get_response( client, conf, uri.as_str()).await?;
    let data_response: DataResponse = from_json( response).await?;
    Ok(data_response.results)
}

pub async fn get_heat_map (client: &Client, conf: &N5Config, device_id: u32, n_last: usize, n_hours: usize)->Result<Vec<HeatMap>> {
    let uri = format!("{}/devices/{}/heat-map?page_size={}&sort_dir=DESC&interval={}-hours", conf.base_uri, device_id, n_last, n_hours);
    let response = get_response( client, conf, uri.as_str()).await?;
    let heat_map_response: HeatMapResponse = from_json( response).await?;
    Ok(heat_map_response.results)
}

pub async fn get_alerts (client: &Client, conf: &N5Config, device_id: u32, n_last: usize)->Result<Vec<Alert>> {
    let uri = format!("{}/devices/{}/alerts?page_size={}&sort_dir=DESC", conf.base_uri, device_id, n_last);
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

/* #endregion queries */
