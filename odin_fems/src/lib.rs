/*
 * Copyright © 2026, United States Government, as represented by the Administrator of
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

// note that we are not using the graphql-client crate here since it puts additional
// constraints on reqwest versions and requires a full schema

use std::{path::{Path,PathBuf}, str::FromStr, time::Duration, collections::HashMap,
    fs::{File,read_to_string}, ops::{Deref,DerefMut}, io::BufReader
};
use axum::http::HeaderValue;
use chrono::{DateTime,Utc,Datelike,Timelike, NaiveTime};
use lazy_static::lazy_static;
use odin_macro::public_struct;
use serde::{Serialize,Deserialize};
use serde_json::{self, value::{Value as JsonValue}};
use reqwest::{Client, header::CONTENT_TYPE};
use struct_field_names_as_array::FieldNamesAsArray;
use strum::{AsRefStr,EnumString};
use structstruck;
use uom::si::f64::{Angle, Length, Pressure, ThermodynamicTemperature, Ratio, Velocity, HeatFluxDensity};
use uom::si::{
    thermodynamic_temperature::{degree_celsius,degree_fahrenheit},
    pressure::{pascal,inch_of_mercury},
    length::{inch},
    velocity::{mile_per_hour},
    ratio::{percent,ratio},
    heat_flux_density::watt_per_square_meter,
};
use itertools::izip;

use odin_build::{pkg_cache_dir,define_load_asset,define_load_config};

use odin_common::{
    angle::{Angle360, Latitude, Longitude},
    collections::{RingDeque,SortedCollection},
    datetime::{Dated, EpochMillis, days, deserialize_duration, full_hour, hours, minutes, secs, to_epoch_millis},
    fs::{ensure_writable_dir, get_filename_extension, odin_data_filename},
    geo::GeoPoint3,
    strings::capitalize_words,
    json_writer::{JsonWriter,JsonWritable,NumFormat},
    net::{header, post_query}
};
use odin_server::{spa::SpaService,ws_service::ws_msg_from_json};

pub mod errors;
use errors::{OdinFemsError,op_failed};
pub type Result<T> = errors::Result<T>;

pub mod actor;
pub mod service;
use service::FemsService;

lazy_static! {
    pub static ref CACHE_DIR: PathBuf = { pkg_cache_dir!() };
}

define_load_config!{}
define_load_asset!{}

/// FEMS stations: https://fems.fs2c.usda.gov
/// queries (graphql): https://wildfireweb-prod-media-bucket.s3.us-gov-west-1.amazonaws.com/s3fs-public/2025-09/FEMS%20Climatology%20API%20External%20User%20Guide.pdf

#[derive(Deserialize,Debug)]
#[public_struct]
struct FemsConfig {
    region: String,
    url: String,
    station_ids: Vec<u32>,
    tx_delay: Duration, // how long to wait between station tx time and retrieval
    check_interval: Duration, // how often to download & check the database for new updates
    forecast_hours: u32, // the number of forecast hours to retrieve
    max_file_age: Duration, // duration after which to delete cache files
}

/// a newtype to wrap a HashMap of configured FemsStation records
pub struct FemsStore (HashMap<u32,FemsStation>);

impl Deref for FemsStore {
    type Target = HashMap<u32,FemsStation>;
    fn deref (&self)-> &Self::Target { &self.0 }
}
impl DerefMut for FemsStore {
    fn deref_mut (&mut self) -> &mut HashMap<u32,FemsStation> { &mut self.0 }
}

impl FemsStore {
    pub fn get_json_snapshot_msg (&self)->String {
        let mut w = JsonWriter::with_capacity(8192 * self.0.len());
        w.write_object( |w| {
            w.write_array_field("stations", |w| {
                for (_k,station) in self.0.iter() {
                    station.write_json_to(w);
                }
            })
        });
        ws_msg_from_json( FemsService::mod_path(), "snapshot", w.as_str())
    }
}

/// this is the main aggregate struct, containing the basic metadata, current weather and fire danger ratingand
/// hourly forecasts for both. The variable parts of the structure are updated at the given tx_* time and interval
#[derive(Debug,Clone)]
#[public_struct]
struct FemsStation {
    id: u32, // this is the station_id || fems_station_id
    name: String,
    agency: String,
    position: GeoPoint3,

    tx_time: NaiveTime,
    tx_frequency: Duration,

    weather_obs: Vec<FemsWeatherObs>,

    nfdrs_obs_v: Vec<FemsNfdrsObs>, // NFDRS obs for V fuel model (grass)
    nfdrs_obs_w: Vec<FemsNfdrsObs>, // NFDRS obs for W fuel model (grass-shrub)
    nfdrs_obs_x: Vec<FemsNfdrsObs>, // NFDRS obs for X fuel model (brush)
    nfdrs_obs_y: Vec<FemsNfdrsObs>, // NFDRS obs for Y fuel model (timber)
    nfdrs_obs_z: Vec<FemsNfdrsObs>, // NFDRS obs for Z fuel model (slash)

    //... more to follow
}

impl FemsStation {
    pub fn nfdrs_obs_for_model (&mut self, fm: NfdrsFuelModel)->&mut Vec<FemsNfdrsObs> {
        match fm {
            NfdrsFuelModel::V => &mut self.nfdrs_obs_v,
            NfdrsFuelModel::W => &mut self.nfdrs_obs_w,
            NfdrsFuelModel::X => &mut self.nfdrs_obs_x,
            NfdrsFuelModel::Y => &mut self.nfdrs_obs_y,
            NfdrsFuelModel::Z => &mut self.nfdrs_obs_z,
        }
    }

    pub fn obs_date (&self)->Option<DateTime<Utc>> {
        self.weather_obs.first().and_then(|obs| if obs.is_forecast { None } else { Some(obs.date) })
    }

    pub fn get_json_update_msg (&self)->String {
        let mut w = JsonWriter::with_capacity(8192);
        w.write_object( |w| self.write_obs_fields_to(w));
        ws_msg_from_json( FemsService::mod_path(), "update", w.as_str())
    }

    //--- the component JSON writers
    pub fn write_metadata_fields_to (&self, w: &mut JsonWriter) {
        w.write_field( "id", self.id);
        w.write_field( "name", &self.name);
        w.write_field( "agency", &self.agency);
        w.write_json_field( "position", &self.position);
        w.write_field( "tx_minute", self.tx_time.minute());
        w.write_field( "tx_interval", self.tx_frequency.as_secs() / 60);
    }

    pub fn write_obs_fields_to (&self, w: &mut JsonWriter) {
        w.write_field( "id", self.id);

        w.write_array_field( "weather_obs", |w| {
            for o in self.weather_obs.iter() {
                o.write_json_to( w);
            }
        });

        w.write_array_field( "nfdrs_obs", |w| {
            for (ov,ow,ox,oy,oz) in izip!( &self.nfdrs_obs_v, &self.nfdrs_obs_w, &self.nfdrs_obs_x, &self.nfdrs_obs_y, &self.nfdrs_obs_z) {
                FemsNfdrsObs::write_json_to( w, ov,ow,ox,oy,oz);
            }
        });
    }
}

impl JsonWritable for FemsStation {
    // this is a full snapshot of the station (metadata plus observations/forecasts)
    fn write_json_to (&self, w: &mut JsonWriter) {
        w.write_object( |w| {
            w.write_object_field( "meta", |w| self.write_metadata_fields_to(w));
            w.write_object_field( "obs", |w| self.write_obs_fields_to(w))
        });
    }

    fn estimated_length (&self)->usize { 8192 }
}

impl TryFrom<RawStationMetaData> for FemsStation {
    type Error = OdinFemsError;

    fn try_from (raw: RawStationMetaData)->Result<Self> {
        Ok( FemsStation {
            id: raw.station_id,
            name: capitalize_words( &raw.station_name.to_lowercase()), // make it more readable
            agency: raw.agency,
            position: GeoPoint3::from_lon_lat_degrees_alt_ft( raw.longitude as f64, raw.latitude as f64, raw.elevation as f64),

            tx_time: NaiveTime::parse_from_str( &raw.transmit_time, "%H:%M:%S")?,
            tx_frequency: minutes( raw.tx_frequency as u64),

            // those are filled in later
            weather_obs: Vec::with_capacity(0),
            nfdrs_obs_v: Vec::with_capacity(0),
            nfdrs_obs_w: Vec::with_capacity(0),
            nfdrs_obs_x: Vec::with_capacity(0),
            nfdrs_obs_y: Vec::with_capacity(0),
            nfdrs_obs_z: Vec::with_capacity(0),
        } )
    }
}

#[derive(Deserialize,Debug,FieldNamesAsArray)]
struct RawStationMetaData {
    fems_station_id: u32, // 55521297,
    station_id: u32, // 44731,
    wims_id: Option<String>, // "44731",
    nesdis_id: Option<String>, // "CA229522",
    wrcc_id: Option<String>, // "CFOU",
    station_name: String, // "FOUNTAIN SPRINGS",
    state: String, // "CA",
    county_name: String, // "Tulare",
    latitude: f32, // 35.89116,
    longitude: f32, // -118.91559,
    elevation: u32, // 794,
    slope: Option<u8>, // 1,
    aspect: Option<String>, // "0": flat, "1"-"8" cardinal direction
    aspect_direction: Option<String>, // "E",
    aspect_degree: Option<String>, // "101",
    kbdi_threshold: u32, // 800,
    init_kbdi: u32, // 100,
    avg_annual_precip: f32, // 9.91,
    annual_maintenance_date_time: String, // "2024-03-5T00:00:00.000Z",
    period_record_start: String, // "2005-01-01T00:00:00.000Z",
    period_record_stop: String, // "2022-12-31T00:00:00.000Z",
    station_status: String, // "A",
    class: String, // "Permanent",
    network_id: u32, // 2,
    network_name: String, // "RAWS",
    agency: String, // "S&PF",
    region: String, // "CALIFORNIA",
    unit: String, // "CDF",
    sub_unit: Option<String>, // "TULARE UNIT",
    ownership_type: String, // "FIRE",
    goes: String, // "West",
    tx_frequency: u32, // 60,
    obs_frequency: u32, // 60,
    site_description: Option<String>, // null,
    maintenance_standard: Option<String>, // "Yes",
    reg_scheduled_observation_time: u32, // 0,
    time_zone: String, // "PST",
    time_zone_offset: i8, // -8,
    zoom_level: Option<u8>, // null,
    transmit_time: String, // "00:00:50",   <<<<<< this is the hourly minute when the station is supposed to report
    modified_time: String, // "2025-03-13T22:40:42.600Z",
    modified_by: String, // "WXx Service",
    //created_time: null,
    //created_by: "EA",
    first_observation: String, // "2005-01-01",
    last_observation: String, // "2025-07-24",
    site: Option<String>, // null,
    observing_agency: Option<String>, // null,
    gsi_parameter_catalog_id: Option<String>, // null,
    //station_type: u32, // 4,
    nfdr_visibility: String, // "True"
}

//--- the GraphQL wrappers
// we get this as { "data": { "stationMetaData": { "data": [ <RawStationMetaData> ] } } }
structstruck::strike! {
    #[structstruck::each[derive(Debug,Deserialize)]]
    #[structstruck::each[serde(rename_all="camelCase")]]
    struct StationMetaDataResponse {
        data: struct StationMetaDataObject {
            station_meta_data: struct StationMetaDataDataObject {
                data: Vec<RawStationMetaData>
            }
        }
    }
}

fn station_metadata_query (station_ids: &[u32])->String {
    let ids = station_ids.iter().map( |s| s.to_string()).collect::<Vec<String>>().join(",");
    let fields = RawStationMetaData::FIELD_NAMES_AS_ARRAY.join("\\n ");
    // this fails for stations that nly have a fems_station_id (e.g. 55522229 Calero)
    //format!(r#"{{ "query": "query StationMetaData {{\n stationMetaData(stationIds: \"{}\"){{\n data {{\n {}\n }}\n }}\n }}" }}"#, ids, fields)
    format!(r#"{{ "query": "query GetStationMetaData($stationId: String!, $hasHistoricData: TriState) {{ stationMetaData(stationIds: $stationId, hasHistoricData: $hasHistoricData){{ data {{ {} }} }} }}", "variables":{{ "stationId":"{}", "hasHistoricData":"ALL" }} }}"#, fields, ids)
}

pub fn metadata_path (station_id: u32)->PathBuf {
    CACHE_DIR.join( format!("{}__station.json", station_id))
}

pub async fn download_station_metadata<P: AsRef<Path>> (client: &Client, url: &str, path: P, station_id: u32)->Result<u64> {
    let request_body = station_metadata_query( &[station_id]);
    let headers = &[(CONTENT_TYPE, HeaderValue::from_static("application/json"))];
    Ok( post_query( client, url, Some(headers), path, request_body).await? )
}

pub async fn download_all_station_metadata (client: &Client, url: &str, region: &str, station_ids: &[u32])->Result<PathBuf> {
    let fname = format!("{}__stations", region);
    let fname = odin_data_filename( &fname, Some( Utc::now()), &[], Some("json"));
    let path = CACHE_DIR.join( fname);
    let request_body = station_metadata_query( station_ids);
    let headers = &[(CONTENT_TYPE, HeaderValue::from_static("application/json"))];

    let len = post_query( client, url, Some(headers), &path, request_body).await?;
    if len > 100 { // account for empty arrays
        Ok( path )
    } else {
        Err( op_failed!("no station metadata downloaded"))
    }
}


/// the high level version of FEMS weather observation data. This is kept in the FemsStation struct and hence
/// does not need to store the associated id of other metadata
#[derive(Debug,Clone)]
#[public_struct]
struct FemsWeatherObs {
    date: DateTime<Utc>, // the observation time
    position: GeoPoint3, // this is always set (non-nullable)
    is_forecast: bool,

    temperature: ThermodynamicTemperature,
    rel_humidity: Ratio,
    hourly_precip: Length, // inch
    sr: HeatFluxDensity, // W/m2

    wind_spd: Velocity,
    wind_dir: Angle360,

    gust_spd: Option<Velocity>,
    gust_dir: Option<Angle360>,

    color: String, // FEMS (CSS hex) color code - TODO should we turn this into an enum? There seems to be no definition

    //... more to follow
}

impl JsonWritable for FemsWeatherObs {
    fn write_json_to (&self, w: &mut JsonWriter) {
        w.write_object( |w| {
            w.write_date_field("date", self.date);
            w.write_json_field("position", &self.position);
            w.write_field("isFc", self.is_forecast);

            w.write_field( "temp", self.temperature.get::<degree_fahrenheit>() as usize);
            w.write_field( "rh", self.rel_humidity.get::<percent>() as usize);
            w.write_f64_field( "hPrecip", self.hourly_precip.get::<inch>(), NumFormat::Fp2 );
            w.write_field( "sr", self.sr.get::<watt_per_square_meter>());

            w.write_f64_field( "wndSpd", self.wind_spd.get::<mile_per_hour>(), NumFormat::Fp1 );
            w.write_f64_field( "wndDir", self.wind_dir.degrees(), NumFormat::Fp0 );

            if let Some(gust_spd) = self.gust_spd {
                w.write_f64_field( "gstSpd", gust_spd.get::<mile_per_hour>(), NumFormat::Fp1 );
            }
            if let Some(gust_dir) = self.gust_dir {
                w.write_f64_field( "gstDir", gust_dir.degrees(), NumFormat::Fp0 );
            }

            w.write_field( "color", &self.color);
        });
    }
}

impl TryFrom<RawWeatherObs> for FemsWeatherObs {
    type Error = OdinFemsError;

    fn try_from (raw: RawWeatherObs)->Result<Self> {
        Ok( FemsWeatherObs {
            date: DateTime::parse_from_rfc3339( &raw.observation_time)?.to_utc(),
            position: GeoPoint3::from_lon_lat_degrees_alt_ft( raw.longitude as f64, raw.latitude as f64, raw.elevation as f64),
            is_forecast: (raw.observation_type == "F"),
            temperature: ThermodynamicTemperature::new::<degree_fahrenheit>(raw.temperature as f64),
            rel_humidity: Ratio::new::<percent>( raw.relative_humidity as f64),
            hourly_precip: Length::new::<inch>( raw.hourly_precip as f64),
            sr: HeatFluxDensity::new::<watt_per_square_meter>( if let Some(sol_rad) = raw.sol_rad {sol_rad as f64} else { 0.0} ),
            wind_spd: Velocity::new::<mile_per_hour>(raw.wind_speed as f64),
            wind_dir: Angle360::from_degrees( raw.wind_direction as f64),
            gust_spd: raw.peak_gust_speed.map( |spd| Velocity::new::<mile_per_hour>(spd as f64)),
            gust_dir: raw.peak_gust_dir.map( |dir| Angle360::from_degrees( dir as f64)),
            color: raw.hex,
        } )
    }
}

/*
 TODO - use enum to map color codes

 --- color codes for common weather risk:
 TSTM (Light Green): General/non-severe storms.
 1-MRGL (Dark Green): Marginal risk.
 2-SLGT (Yellow): Slight risk.
 3-ENH (Orange): Enhanced risk.
 4-MDT (Red): Moderate risk.
 5-HIGH (Magenta): High risk

 --- specific threats:
 Tornado Warning (Red): #FF0000
 Tsunami Warning (Tomato/Orange-Red): #FD6347
 */

/// as we get it from the JSON response
/// according to https://wildfireweb-prod-media-bucket.s3.us-gov-west-1.amazonaws.com/s3fs-public/2025-09/FEMS Climatology API%20External User Guide.pdf
#[derive(Deserialize,Debug,FieldNamesAsArray)]
struct RawWeatherObs {
    station_id: u32,
    //wrcc_id: String, // we don't need it
    //station_name: String, // we don't need it
    latitude: f32,
    longitude: f32,
    elevation: f32,
    //zoom_level: Option<u32>, // we don't need it
    station_type: Option<String>,
    observation_time: String, // "2024-10-08T00:37:09.000Z",
    //observation_time_lst: "2024-10-07T18:37:09-06:00",
    //observation_time_offset: String, // "2024-10-07T20:37:09-04:00",
    //display_hour: String, // calc: "2024-10-08T01:00:00.000Z",
    //display_hour_lst: String, // calc: "2024-10-07T19:00:00-06:00",
    //display_hour_offset: String, // calc: "2024-10-07T21:00:00-04:00",
    //display_date: String, // calc: "2024-10-07",

    // note the data fields are all nullable

    temperature: i32,
    relative_humidity: u32,
    hourly_precip: f32,
    //hr24Precipitation: null, // deprecated
    //hr48Precipitation: null, // deprecated
    //hr72Precipitation: null, // deprecated
    wind_speed: u32,
    wind_direction: u32,
    peak_gust_speed: Option<u32>,
    peak_gust_dir: Option<u32>,
    sol_rad: Option<f32>,
    snow_flag: Option<String>, // "Y": yes, "N": no,
    observation_type: String, // "O": observation, "F": forecast,
    hex: String,  // hex color value associated with measured attr e.g. "#0c9687"
    t_flag: Option<u8>, // temperature QC flag:  0: orig value retained, 1: orig val estimated, 2: suspicious
    rh_flag: Option<u8>, // rel humidity QC flag
    pcp_flag: Option<u8>, // precipitation QC flag
    ws_flag: Option<u8>, // windspeed QC flag
    wa_flag: Option<u8>, // wind azimuth QC flag
    sr_flag: Option<u8>, // solar radiation QC flag
    gs_flag: Option<u8>, // wind gust speed QC flag
    ga_flag: Option<u8>, // wind gust azimuth QC flag
}

//--- the GraphQL wrappers
// we get this as { "data": { "weatherObs": { "data": [ <RawWeatherObs> ] } } }
structstruck::strike! {
    #[structstruck::each[derive(Debug,Deserialize)]]
    #[structstruck::each[serde(rename_all="camelCase")]]
    struct WeatherObsResponse {
        data: struct WeatherObsObject {
            weather_obs: struct WeatherObsDataObject {
                data: Vec<RawWeatherObs>
            }
        }
    }
}

/// the FEMS server uses GraphQL queries with JSON encoded POST bodies
fn weather_obs_query (station_ids: &[u32], start: DateTime<Utc>, end: DateTime<Utc>)->String {
    let ids = station_ids.iter().map( |s| s.to_string()).collect::<Vec<String>>().join(",");
    let start = start.format("%Y-%m-%dT%H:%M:%SZ");
    let end = end.format("%Y-%m-%dT%H:%M:%SZ");
    let fields = RawWeatherObs::FIELD_NAMES_AS_ARRAY.join("\\n ");
    //format!(r#"{{ "query": "query GetWeatherObs {{ weatherObs(stationIds: \"{}\"\n startDateTimeRange: \"{}\"\n endDateTimeRange: \"{}\"){{ data {{ {} }} }} }}" }}"#,
    //    ids, start, end, fields)
    format!(r#"{{ "query": "query GetWeatherObs( $stationId: String!, $startDate: DateTime!, $endDate:DateTime!, $hasHistoricData: TriState) {{ weatherObs(stationIds: $stationId, startDateTimeRange: $startDate, endDateTimeRange: $endDate, hasHistoricData: $hasHistoricData){{ data {{ {} }} }} }}", "variables":{{ "stationId":"{}", "startDate":"{}", "endDate": "{}", "hasHistoricData":"ALL" }} }}"#,
        fields, ids, start, end)
}

pub async fn download_weather_obs<P: AsRef<Path>> (client: &Client, url: &str, path: P, station_id: u32, start: DateTime<Utc>, end: DateTime<Utc>)->Result<u64> {
    let request_body = weather_obs_query( &[station_id], start, end);
    //println!("@@@ weather obs query:\n{}\n", request_body);

    let headers = &[header("content-type", "application/json")];
    Ok( post_query( client, url, Some(headers), path, request_body).await? )
}

pub async fn download_all_weather_obs (client: &Client, url: &str, region: &str, station_ids: &[u32], start: DateTime<Utc>, end: DateTime<Utc>)->Result<PathBuf> {
    let fname = format!("{}__weather", region);
    let fname = odin_data_filename( &fname, Some( start), &[], Some("json"));
    let path = CACHE_DIR.join( fname);
    let request_body = weather_obs_query( station_ids, start, end);
    let headers = &[(CONTENT_TYPE, HeaderValue::from_static("application/json"))];

    let len = post_query( client, url, Some(headers), &path, request_body).await?;
    if len > 100 { // account for empty arrays
        Ok( path )
    } else {
        Err( op_failed!("no weather obs data downloaded"))
    }
}

pub fn station_weather_obs_path (station_id: u32, ref_time: DateTime<Utc>, forecast_hours: u32)->PathBuf {
    let fname = format!("{}__weather", station_id);
    let fname = odin_data_filename( &fname, Some( ref_time), &[], Some("json"));
    CACHE_DIR.join( fname)
}

pub fn obs_timeframe (base_time: DateTime<Utc>, forecast_hours: u32)->(DateTime<Utc>,DateTime<Utc>) {
    let start = full_hour(&base_time) - hours(1); // make sure we get the last observation (not just forecasts)
    let end = base_time + hours( forecast_hours as u64); // note this applies forecast hours to basetime, not start
    (start, end)
}

/// see https://www.fs.usda.gov/rm/pubs_journals/2024/rmrs_2024_jolly_m001.pdf
#[derive(AsRefStr, EnumString, Debug, Clone)]
pub enum NfdrsFuelModel {
    V, // grass
    W, // grass-shrub
    X, // brush
    Y, // timber
    Z, // slash
}

pub const ALL_FUEL_MODELS: &[NfdrsFuelModel] = &[NfdrsFuelModel::Y, NfdrsFuelModel::W, NfdrsFuelModel::X, NfdrsFuelModel::Z, NfdrsFuelModel::V ];

/// high level representation of fire danger rating observation. Kept within FemsStation struct and
/// hence does not need to store metadata
#[derive(Debug,Clone)]
#[public_struct]
struct FemsNfdrsObs {
    date: DateTime<Utc>,
    is_forecast: bool,
    fuel_model: NfdrsFuelModel,

    kbdi: u32, // Keech Bryam drought index

    dfm_1h: Ratio,
    dfm_10h: Ratio,
    dfm_100h: Ratio,
    dfm_1000h: Ratio,

    lfm_herb: Ratio,
    lfm_wood: Ratio,

    ic: f32, // ignition component
    sc: f32, // spread component
    erc: f32, // energy release component

    bi: f32, // burning index
    gsi: f32, // growing season index

    //... possibly more to follow
}

impl FemsNfdrsObs {
    pub fn write_json_to (w: &mut JsonWriter, obs_v: &FemsNfdrsObs, obs_w: &FemsNfdrsObs, obs_x: &FemsNfdrsObs, obs_y: &FemsNfdrsObs, obs_z: &FemsNfdrsObs) {
        w.write_object( |w| {
            w.write_date_field( "date", obs_y.date);
            w.write_field( "isFc", obs_y.is_forecast);

            //those are the same for all fuel model records
            w.write_field( "kbdi", obs_y.kbdi);
            w.write_f32_field( "gsi", obs_y.gsi, NumFormat::Fp2);
            w.write_f64_field( "dfm1h", obs_y.dfm_1h.get::<percent>(), NumFormat::Fp0);
            w.write_f64_field( "dfm10h", obs_y.dfm_10h.get::<percent>(), NumFormat::Fp0);
            w.write_f64_field( "dfm100h", obs_y.dfm_100h.get::<percent>(), NumFormat::Fp0);
            w.write_f64_field( "lfmHerb", obs_y.lfm_herb.get::<percent>(), NumFormat::Fp0);
            w.write_f64_field( "lfmWood", obs_y.lfm_wood.get::<percent>(), NumFormat::Fp0);

            w.write_f32_array_field( "ic", &[obs_v.ic, obs_w.ic, obs_x.ic, obs_y.ic, obs_z.ic], NumFormat::Fp2);
            w.write_f32_array_field( "sc", &[obs_v.sc, obs_w.sc, obs_x.sc, obs_y.sc, obs_z.sc], NumFormat::Fp2);
            w.write_f32_array_field( "erc", &[obs_v.erc, obs_w.erc, obs_x.erc, obs_y.erc, obs_z.erc], NumFormat::Fp2);
            w.write_f32_array_field( "bi", &[obs_v.bi, obs_w.bi, obs_x.bi, obs_y.bi, obs_z.bi], NumFormat::Fp2);
        })
    }
}

impl TryFrom<RawNfdrsObs> for FemsNfdrsObs {
    type Error = OdinFemsError;

    fn try_from (raw: RawNfdrsObs)->Result<Self> {
        let now = Utc::now();
        let date = DateTime::parse_from_rfc3339( &raw.observation_time)?.to_utc();

        let is_forecast = if let Some(obs_type) = raw.observation_type {
            &obs_type == "F"
        } else {
            date > now
        };

        Ok( FemsNfdrsObs {
            date,
            is_forecast,

            fuel_model: NfdrsFuelModel::from_str( &raw.fuel_model)?,
            kbdi: raw.kbdi,

            dfm_1h: Ratio::new::<percent>( raw.one_hr_tl_fuel_moisture as f64),
            dfm_10h: Ratio::new::<percent>( raw.ten_hr_tl_fuel_moisture as f64),
            dfm_100h: Ratio::new::<percent>( raw.hun_hr_tl_fuel_moisture as f64),
            dfm_1000h: Ratio::new::<percent>( raw.thou_hr_tl_fuel_moisture as f64),

            lfm_herb: Ratio::new::<percent>( raw.herbaceous_lfi_fuel_moisture as f64),
            lfm_wood: Ratio::new::<percent>( raw.woody_lfi_fuel_moisture as f64),

            ic: raw.ignition_component,
            sc: raw.spread_component,
            erc: raw.energy_release_component,

            bi: raw.burning_index,
            gsi: raw.gsi,
        } )
    }
}

/// National Fire Danger Rating System data as we get it from the JSON response
/// according to https://wildfireweb-prod-media-bucket.s3.us-gov-west-1.amazonaws.com/s3fs-public/2025-09/FEMS Climatology API%20External User Guide.pdf
#[derive(Deserialize,Debug,FieldNamesAsArray)]
struct RawNfdrsObs {
    station_id: u32,
    observation_time: String,  // ?? TODO - GraphQL spec says this is optional ??
    nfdr_date: String,  // YYYY-MM-dd
    nfdr_time: u32,
    nfdr_type: String,  // O: observation, F: forecast
    fuel_model: String, // V: , W: , X: , Y: , Z:
    // fuel_model_version: String,
    kbdi: u32, // Keech Bryam Drought Index (0-200: low, 200-400: moderate, 400-600: hight, 600-800: extreme)
    one_hr_tl_fuel_moisture: f32,  // 1h time lag dead fuel moisture (<0.25")
    ten_hr_tl_fuel_moisture: f32,  // 10h time lag dead fuel moisture (<1")
    hun_hr_tl_fuel_moisture: f32,  // 100h time lag dead fuel moisture (<3")
    thou_hr_tl_fuel_moisture: f32,  // 1000h time lag dead fuel moisture (>3")
    ignition_component: f32, // probability that firebrand (ember) will start fire requiring suppression
    spread_component: f32, // theoretical forward spread of headfire (ft/min)
    energy_release_component: f32, // total available energy that can be released by headfire (BTU/sqft)
    burning_index: f32, // upper limit of expected fire intensity  containment difficulty + (flame length * 10) + intensity
    herbaceous_lfi_fuel_moisture: f32, // live herbaceous fuel moisture
    woody_lfi_fuel_moisture: f32, // live woody fuel moisture
    gsi: f32, // growing season index (0: inactive season - 1: full growth)
    observation_type: Option<String>, // O: observation, F: forecast
}

//--- the GraphQL wrappers
// we get this as { "data": { "nfdrsObs": { "data": [ <RawNfdrsObs> ] } } }
structstruck::strike! {
    #[structstruck::each[derive(Debug,Deserialize)]]
    #[structstruck::each[serde(rename_all="camelCase")]]
    struct NfdrsObsResponse {
        data: struct NfdrsObsObject {
            nfdrs_obs: struct NfdrsObsDataObject {
                data: Vec<RawNfdrsObs>
            }
        }
    }
}

fn nfdrs_obs_query (station_ids: &[u32], start: DateTime<Utc>, end: DateTime<Utc>, fuel_models: &[NfdrsFuelModel])->String {
    let ids = station_ids.iter().map( |s| s.to_string()).collect::<Vec<String>>().join(",");
    let fms = fuel_models.iter().map( |s| s.as_ref()).collect::<Vec<&str>>().join(",");

    // start/end hour are for each covered day so if we extend to the next day we unfortunately have to retrieve all hours
    let dh = (end - start).num_hours() as u32;
    let (h0,h1) = if start.hour() + dh > 24 { (0,24) } else { (start.hour(), end.hour()) };

    let start_date = start.format("%Y-%m-%d");
    let end_date = end.format("%Y-%m-%d");
    let fields = RawNfdrsObs::FIELD_NAMES_AS_ARRAY.join("\\n ");
    //format!(r#"{{ "query": "query NfdrsObs {{ nfdrsObs(stationIds: \"{}\"\n nfdrType: \"Appended\"\n startDateRange: \"{}\"\n endDateRange: \"{}\"\n startHour: {}\n endHour: {}\n fuelModels: \"{}\"){{ data {{ {} }} }} }}" }}"#,
    //    ids, start_date, end_date, start.hour(), end.hour(), fuel_model.as_ref(), fields)

    // note that [startHour..endHour] is a daily interval (not only for first and last day)
    format!(r#"{{ "query": "query GetNfdrHourly($stationIds: String, $hasHistoricData: TriState) {{ nfdrsObs(stationIds: $stationIds, nfdrType: \"Appended\", startDateRange: \"{}\", endDateRange: \"{}\", startHour: {}, endHour: {}, fuelModels: \"{}\", hasHistoricData: $hasHistoricData){{ data {{ {} }} }} }}", "variables":{{ "stationIds":"{}", "hasHistoricData":"ALL" }} }}"#,
        start_date, end_date, h0, h1, fms, fields, ids)
}

pub async fn download_nfdrs_obs<P: AsRef<Path>> (client: &Client, url: &str, path: P, station_id: u32, start: DateTime<Utc>, end: DateTime<Utc>, fuel_models: &[NfdrsFuelModel])->Result<u64> {
    let request_body = nfdrs_obs_query( &[station_id], start, end, fuel_models);
    let headers = &[header("content-type", "application/json")];
    Ok( post_query( client, url, Some(headers), path, request_body).await? )
}

pub fn station_nfdrs_obs_path (station_id: u32, ref_time: DateTime<Utc>, forecast_hours: u32, fuel_models: &[NfdrsFuelModel])->PathBuf {
    let fms = fuel_models.iter().map( |s| s.as_ref()).collect::<Vec<&str>>().join("");
    let prefix = format!("{}__nfdrs", station_id);
    let fname = odin_data_filename( &prefix, Some( ref_time), &[ &fms, &forecast_hours.to_string() ], Some("json"));
    CACHE_DIR.join( fname)
}

pub async fn download_all_nfdrs_obs (client: &Client, url: &str, region: &str, station_ids: &[u32], start: DateTime<Utc>, end: DateTime<Utc>, fuel_models: &[NfdrsFuelModel])->Result<PathBuf> {
    let fms = fuel_models.iter().map( |s| s.as_ref()).collect::<Vec<&str>>().join("");
    let fname = format!("{}__nfdrs", region);
    let fname = odin_data_filename( &fname, Some( start), &[ &fms ], Some("json"));
    let path = CACHE_DIR.join( fname);
    let request_body = nfdrs_obs_query( station_ids, start, end, ALL_FUEL_MODELS);
    let headers = &[(CONTENT_TYPE, HeaderValue::from_static("application/json"))];

    let len = post_query( client, url, Some(headers), &path, request_body).await?;
    if len > 100 { // account for empty arrays
        Ok( path )
    } else {
        Err( op_failed!("no nfdrs obs data downloaded"))
    }
}

// make sure we don't bail prematurely - as long as there is one station we still return Ok
pub async fn get_stations (client: &Client, config: &FemsConfig)->Result<FemsStore> {
    let mut stations: HashMap<u32,FemsStation> = HashMap::new();

    match download_all_station_metadata( client, &config.url, &config.region, &config.station_ids).await {
        Ok(path) => {
            let reader = BufReader::new( File::open( &path)?);
            let resp: StationMetaDataResponse = serde_json::from_reader(reader)?;
            for raw in resp.data.station_meta_data.data.into_iter() {
                let id = raw.station_id;
                match FemsStation::try_from(raw) {
                    Ok(station) => {
                        stations.insert( id, station);
                    }
                    Err(e) => { eprintln!("failed to convert raw data for station: {}: {}", id, e); }
                }
            }
        }
        Err(e) => { eprintln!("error downloading metadata for configured stations: {}", e); }
    }

    if stations.is_empty() {
        eprintln!("no station data retrieved");
        Err( op_failed!("no station data retrieved"))
    } else {
        let mut store = FemsStore(stations);
        get_current_weather_obs( &client, config, &mut store).await?;
        get_current_nfdrs_obs(client, config, &mut store).await?;

        Ok( store )
    }
}

pub async fn get_current_weather_obs (client: &Client, config: &FemsConfig, store: &mut FemsStore)->Result<()> {
    let now = Utc::now();
    let start = now - hours(2);
    let end = now + hours(config.forecast_hours as u64);

    match download_all_weather_obs( client, &config.url, &config.region, &config.station_ids, start, end).await {
        Ok(path) => {
            let reader = BufReader::new( File::open( &path)?);
            let resp: WeatherObsResponse = serde_json::from_reader(reader)?;

            for (k,v) in store.iter_mut() {
                v.weather_obs = Vec::with_capacity( config.forecast_hours as usize + 2);
            }

            for raw in resp.data.weather_obs.data.into_iter() {
                let id = raw.station_id;
                if let Some(station) = store.get_mut(&id) {
                    match FemsWeatherObs::try_from(raw) {
                        Ok(obs) => {
                            // we only keep the latest actual observation
                            if !obs.is_forecast && !station.weather_obs.is_empty() && !station.weather_obs[0].is_forecast {
                                if station.weather_obs[0].date < obs.date {
                                    station.weather_obs[0] = obs;
                                } // otherwise we ignore obs
                                continue;
                            }
                            station.weather_obs.sort_in( obs, |a,b| { a.date < b.date })
                        }
                        Err(e) => { eprintln!("failed to convert raw weather data for station: {}: {}", id, e); }
                    }
                } else {
                    eprintln!("ignoring weather obs for unknown station {}", id)
                }
            }
        }
        Err(e) => { eprintln!("error downloading current weather observation for configured stations: {}", e); }
    }

    Ok(())
}

pub async fn get_current_nfdrs_obs (client: &Client, config: &FemsConfig, store: &mut FemsStore)->Result<()> {
    let now = Utc::now();
    let start = now - hours(2);
    let end = now + hours(config.forecast_hours as u64);

    match download_all_nfdrs_obs( client, &config.url, &config.region, &config.station_ids, start, end, ALL_FUEL_MODELS).await {
        Ok(path) => {
            let reader = BufReader::new( File::open( &path)?);
            let resp: NfdrsObsResponse = serde_json::from_reader(reader)?;

            let n_obs = config.forecast_hours as usize + 2;
            for (k,v) in store.iter_mut() {
                v.nfdrs_obs_v = Vec::with_capacity( n_obs);
                v.nfdrs_obs_w = Vec::with_capacity( n_obs);
                v.nfdrs_obs_x = Vec::with_capacity( n_obs);
                v.nfdrs_obs_y = Vec::with_capacity( n_obs);
                v.nfdrs_obs_z = Vec::with_capacity( n_obs);
            }

            for raw in resp.data.nfdrs_obs.data.into_iter() {
                let id = raw.station_id;
                if let Some(station) = store.get_mut(&id) { // is this a known station
                    if let Ok(fm) = NfdrsFuelModel::from_str( &raw.fuel_model) { // is this a known fuel model
                        match FemsNfdrsObs::try_from(raw) {
                            Ok(obs) => {
                                // only add if this is within the time window (NFDRS queries don't let us specify specific start/end dates)
                                if obs.date >= start && obs.date <= end {
                                    let nfdrs_obs = station.nfdrs_obs_for_model(fm);

                                    // only keep the latest actual observation
                                    if !obs.is_forecast && !nfdrs_obs.is_empty() && !nfdrs_obs[0].is_forecast {
                                        if nfdrs_obs[0].date < obs.date {
                                            nfdrs_obs[0] = obs;
                                        } // otherwise we ignore obs
                                        continue;
                                    }
                                    nfdrs_obs.sort_in( obs, |a,b| { a.date < b.date })
                                }
                            }
                            Err(e) => { eprintln!("failed to convert raw NFDRS data for station: {}: {}", id, e); }
                        }
                    } else { eprintln!("ignoring NFDRS obs for fuel model {} (station {})", raw.fuel_model, id) }
                } else { eprintln!("ignoring NFDRS obs for unknown station {}", id) }
            }
        }
        Err(e) => { eprintln!("error downloading current NFDRS observation for configured stations: {}", e); }
    }

    Ok(())
}

pub async fn update_station_weather_obs (client: &Client, config: &FemsConfig, station: &mut FemsStation, start: DateTime<Utc>)->Result<()> {
    let id = station.id;
    let path = station_weather_obs_path( id, start, config.forecast_hours);
    let end = start + hours(config.forecast_hours as u64);

    let len = download_weather_obs( client, &config.url, &path, id, start, end).await?;
    if len < 100 {
        return Err( errors::op_failed!("no weather observation data for station {}", id))
    }

    let reader = BufReader::new( File::open( &path)?);
    let resp: WeatherObsResponse = serde_json::from_reader(reader)?;

    let mut new_obs: Vec<FemsWeatherObs> = Vec::with_capacity(config.forecast_hours as usize + 2);

    for raw in resp.data.weather_obs.data.into_iter() {
        let obs = FemsWeatherObs::try_from(raw)?;
        // we only keep the latest actual observation
        if !obs.is_forecast && !new_obs.is_empty() && !new_obs[0].is_forecast {
            if new_obs[0].date < obs.date {
                new_obs[0] = obs;
            } // otherwise we ignore obs
            continue;
        }
        new_obs.sort_in( obs, |a,b| { a.date < b.date })
    }

    station.weather_obs = new_obs;
    Ok(())
}

pub async fn update_station_nfdrs_obs (client: &Client, config: &FemsConfig, station: &mut FemsStation, start: DateTime<Utc>)->Result<()> {
    let id = station.id;
    let path = station_nfdrs_obs_path( id, start, config.forecast_hours, ALL_FUEL_MODELS);
    let end = start + hours(config.forecast_hours as u64);

    let len = download_nfdrs_obs( client, &config.url, &path, id, start, end, ALL_FUEL_MODELS).await?;
    if len < 100 {
        return Err( errors::op_failed!("no NFDRS observation data for station {}", id))
    }

    let reader = BufReader::new( File::open( &path)?);
    let resp: NfdrsObsResponse = serde_json::from_reader(reader)?;

    let n_obs = config.forecast_hours as usize + 2;
    let mut new_obs_v: Vec<FemsNfdrsObs> = Vec::with_capacity( n_obs);
    let mut new_obs_w: Vec<FemsNfdrsObs> = Vec::with_capacity( n_obs);
    let mut new_obs_x: Vec<FemsNfdrsObs> = Vec::with_capacity( n_obs);
    let mut new_obs_y: Vec<FemsNfdrsObs> = Vec::with_capacity( n_obs);
    let mut new_obs_z: Vec<FemsNfdrsObs> = Vec::with_capacity( n_obs);

    for raw in resp.data.nfdrs_obs.data.into_iter() {
        let obs = FemsNfdrsObs::try_from(raw)?;
        let nfdrs_obs = match obs.fuel_model {
            NfdrsFuelModel::V => &mut new_obs_v,
            NfdrsFuelModel::W => &mut new_obs_w,
            NfdrsFuelModel::X => &mut new_obs_x,
            NfdrsFuelModel::Y => &mut new_obs_y,
            NfdrsFuelModel::Z => &mut new_obs_z,
        };

        // only keep the latest actual observation
        if !obs.is_forecast && !nfdrs_obs.is_empty() && !nfdrs_obs[0].is_forecast {
            if nfdrs_obs[0].date < obs.date {
                nfdrs_obs[0] = obs;
            } // otherwise we ignore obs
            continue;
        }
        nfdrs_obs.sort_in( obs, |a,b| { a.date < b.date })
    }

    station.nfdrs_obs_v = new_obs_v;
    station.nfdrs_obs_w = new_obs_w;
    station.nfdrs_obs_x = new_obs_x;
    station.nfdrs_obs_y = new_obs_y;
    station.nfdrs_obs_z = new_obs_z;

    Ok(())
}
