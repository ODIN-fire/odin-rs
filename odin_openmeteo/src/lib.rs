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

use std::{
    time::Duration, fmt::Debug,
    path::{Path,PathBuf}, ops::Range,
    io::{Read,BufReader}, fs::{File},
    collections::HashMap,
    hash::{Hash,DefaultHasher,Hasher},
    ops::Deref,
    fs
};
use chrono::{DateTime,Utc,Datelike,Timelike,NaiveDate,NaiveTime, NaiveDateTime};
use serde::{Deserialize,Serialize, de::{Deserialize as DeserializeTrait}};
use serde_json;
use gdal::{Metadata, raster::{GdalType, RasterCreationOptions}};
use lazy_static::lazy_static;
use regex::Regex;

use odin_build::define_load_config;
use odin_macro::public_struct;

use odin_common::{
    fs::{odin_data_filename,ensure_writable_dir, filepath_contents_as_string},
    geo::{GeoPoint, GeoRect},
    num_type::NumericType,
    sliced, utm,
    datetime::{as_days, as_minutes},
};
use odin_gdal::{Dataset, get_driver_by_name, get_driver_name_for_extension, grid::{self, GdalGridAlgorithmOptions, create_grid_ds}, is_gtiff_driver, set_band_meta};
use odin_wx::{WxService,WxDataSetRequest,WxFileAvailable,AddDataSet,RemoveDataSet};

pub mod wx_service;
pub mod dataset;

pub mod ids;
use ids::{FieldId,ModelId};

pub mod errors;
use errors::op_failed;

pub mod actor;
pub use actor::OpenMeteoActor;
pub use actor::OpenMeteoActorMsg;

pub mod convert;

use crate::errors::OdinOpenMeteoError;
pub type Result<T> = std::result::Result<T,errors::OdinOpenMeteoError>;

pub const NOAA_HRRR_MODEL: &'static str = "gfs_hrrr";
pub const ECMWF_IFS_MODEL: &'static str = "ecmwf_ifs";
//... and more to follow

/// the Dataset metainfo key for the forecast time
pub const FORECAST_EPOCH: &'static str = "forecast_epoch";

/// the Dataset metainfo key for the forecast time step (usually number of hours since base time)
pub const FORECAST_STEP: &'static str = "forecast_step";

lazy_static! {
    static ref TIMES_RE: Regex = Regex::new( r#""time":\[(.*?)\]"#).unwrap();

    static ref CACHE_DIR: PathBuf = {
        let path = odin_build::cache_dir().join("odin_openmeteo");
        ensure_writable_dir(&path).expect( &format!("invalid Open-Meteo cache dir: {path:?}"));
        path
    };
}

define_load_config!{}

#[derive(Deserialize,Debug)]
pub struct OpenMeteoConfig {
    pub forecast_url: String, // the model independent invariant data url part
    pub archive_url: String,
    pub meta_url: String, // the model independent invariant meta url part

    pub initial_delay: Duration, // to apply on top of the computed schedule
    pub retry_delay: Duration, // between retries of not-yet-available downloads

    /// how long to keep cached response files
    pub max_age: Duration,
}

// the different temporal categories we can retrieve through OpenMeteo
pub enum OpenMeteoTemporal {
    Current, Minutely, Hourly, Daily
}

/* #region URL composition ***************************************************************************************************/

pub fn forecast_url (config: &OpenMeteoConfig, query: &str)->String {
    format!("{}?{}", config.forecast_url, query)
}

pub fn archive_url (config: &OpenMeteoConfig, query: &str)->String {
    format!("{}?{}", config.archive_url, query)
}

pub fn meta_url (config: &OpenMeteoConfig, model_name: &str)->String {
    format!("{}/{}/static/meta.json", config.meta_url, model_name)
}

pub fn url_query_minutely_bbox<T:OpenMeteoDataSet> (bbox: &GeoRect, fc_duration: Duration, model: &str)->String {
    let fc_steps = ((as_minutes( fc_duration) / 15.0).round() as u32).min(32); // max 8h
    format!("models={}&bounding_box={},{},{},{}&cell_selection=nearest&wind_speed_unit=ms&forecast_minutely_15={}&minutely_15={}",
        model,
        bbox.south().degrees(), bbox.west().degrees(), bbox.north().degrees(), bbox.east().degrees(),
        fc_steps,
        T::fields_query()
    )
}

// https://api.open-meteo.com/v1/forecast?
//    latitude=37.142&longitude=-121.2&hourly=temperature_2m,relative_humidity_2m,precipitation,wind_speed_10m,wind_direction_10m,shortwave_radiation,snow_depth&models=gfs_hrrr&forecast_days=1&wind_speed_unit=mph&temperature_unit=fahrenheit&precipitation_unit=inch"

pub fn url_query_hourly_point<T:OpenMeteoDataSet> (pos: &GeoPoint, days: u32, past_days: u32, model: &str)->String {
    format!("latitude={}&longitude={}&models={}&forecast_days={}&past_days={}&hourly={}",
        pos.latitude_degrees(), pos.longitude_degrees(), model, days, past_days, T::fields_query())
}

pub fn url_query_hourly_points<T:OpenMeteoDataSet> (pos: &[GeoPoint], days: u32, past_days: u32, model: &str)->String {
    let lats = pos.iter().map( |p| p.latitude_degrees().to_string()).collect::<Vec<String>>().join(",");
    let lons = pos.iter().map( |p| p.longitude_degrees().to_string()).collect::<Vec<String>>().join(",");

    format!("latitude={}&longitude={}&models={}&forecast_days={}&past_days={}&hourly={}",
        lats, lons, model, days, past_days, T::fields_query())
}

pub fn url_query_hourly_bbox<T:OpenMeteoDataSet> (bbox: &GeoRect, days: u32, past_days: u32, model: &str)->String {
    format!("models={}&bounding_box={},{},{},{}&forecast_days={}&past_days={}&cell_selection=nearest&wind_speed_unit=ms&hourly={}",
        model,
        bbox.south().degrees(), bbox.west().degrees(), bbox.north().degrees(), bbox.east().degrees(),
        days, past_days,
        T::fields_query()
    )
}

// https://archive-api.open-meteo.com/v1/archive?
//   latitude=37.142&longitude=-121.2&start_date=2025-07-01&end_date=2026-06-30&daily=precipitation_sum&precipitation_unit=inch

pub fn url_query_daily_point<T:OpenMeteoDataSet> (pos: &GeoPoint, start: &DateTime<Utc>, end: &DateTime<Utc>)->String {
    let start_date = start.format("%Y-%m-%d");
    let end_date = end.format("%Y-%m-%d");

    format!("latitude={}&longitude={}&start_date={}&end_date={}&daily={}",
        pos.latitude_degrees(), pos.longitude_degrees(), start_date, end_date, T::fields_query())
}

pub fn url_query_daily_points<T:OpenMeteoDataSet> (pos: &[&GeoPoint], start: &DateTime<Utc>, end: &DateTime<Utc>)->String {
    let start_date = start.format("%Y-%m-%d");
    let end_date = end.format("%Y-%m-%d");

    let lats = pos.iter().map( |p| p.latitude_degrees().to_string()).collect::<Vec<String>>().join(",");
    let lons = pos.iter().map( |p| p.longitude_degrees().to_string()).collect::<Vec<String>>().join(",");

    format!("latitude={}&longitude={}&start_date={}&end_date={}&daily={}",
        lats, lons, start_date, end_date, T::fields_query())
}

/* #endregion URL composition */


/// the OpenMeteo response to metadata queries (see https://open-meteo.com/en/docs/model-updates)
/// projected next update is scheduled at t + dt
#[derive(Deserialize,Debug,PartialEq,Eq,Clone)]
#[public_struct]
struct OpenMeteoMetadata {
    chunk_time_length: u32,
    crs_wkt: String,
    data_end_time: i64,
    last_run_availability_time: i64,  // (t)
    last_run_initialisation_time: i64,
    last_run_modification_time: i64,
    temporal_resolution_seconds: i64,
    update_interval_seconds: i64  // (dt)
}

impl OpenMeteoMetadata {
    pub fn next_update (&self)->Result<DateTime<Utc>> {
        DateTime::from_timestamp( self.last_run_availability_time + self.update_interval_seconds, 0)
            .ok_or( op_failed!("invalid meta last_run_availability_time"))
    }

    pub fn base_date (&self)->Result<DateTime<Utc>> {
        DateTime::from_timestamp( self.last_run_initialisation_time, 0)
            .ok_or( op_failed!("invalid meta last_run_initialization_time"))
    }

    pub fn forecasts (&self, basedate: DateTime<Utc>, fc_duration: Duration)->Vec<DateTime<Utc>> {
        let n = (fc_duration.as_secs() as usize / self.temporal_resolution_seconds as usize) + 1; // it always includes the basedate
        let inc = Duration::from_secs( self.temporal_resolution_seconds as u64);
        let end_date = basedate + fc_duration;

        let mut fcs = Vec::with_capacity(n);
        let mut d = basedate;
        while d < end_date {
            fcs.push(d);
            d += inc;
        }

        fcs
    }
}


/// common interface for current/hourly/minutely_15 data records
/// this is the main abstraction for a specific set of fields to retrieve
/// note this does NOT imply the temporal category (current/minutely/hourly/daily) of how this data set is used
pub trait OpenMeteoDataSet {
    // we prefer the query strings over a list of FieldIds since query params might differ. Used only to construct request URLs
    fn fields_query ()->&'static str;
    fn units_query ()->&'static str {
        "wind_speed_unit=ms" // per default precipitation is in [mm], temperature in [°C] and there are no query params to set defaults
    }
    fn dataset_name ()->&'static str;
    fn time_iter (&self)-> impl Iterator<Item=&String>;
    fn n_time_steps (&self)->usize;
    fn nth_time_step (&self, i: usize)->Result<DateTime<Utc>>;

    fn time_steps (&self)->Result<Vec<DateTime<Utc>>> {
        self.time_iter().map( |ts| utc_from_timespec(ts.as_str())).collect()
    }
}

/// struct for OpenMeteo response records, which are normally list of OpenMeteo records that
/// depending on dataset specific query parameters contain current (single time point) or
/// minutely/hourly/daily (multi-time point) values.
/// Note this is structured so that we can use a single generic struct for serialization of
/// all types of location datasets, which means we heavily rely on flattening fields in serde
#[derive(Deserialize,Debug)]
#[public_struct]
struct OpenMeteoLocationData<T> where T: OpenMeteoDataSet {
    #[serde(flatten)]
    metadata: LocationMetaData,

    #[serde(flatten)]
    current_data: Option<CurrentData<T>>,

    #[serde(flatten)]
    minutely_data: Option<MinutelyData<T>>,

    #[serde(flatten)]
    hourly_data: Option<HourlyData<T>>,

    #[serde(flatten)]
    daily_data: Option<DailyData<T>>
}

impl <T> OpenMeteoLocationData<T> where T: OpenMeteoDataSet + for <'a> Deserialize<'a> {
    fn filename (bbox: &GeoRect, date: DateTime<Utc>, fc_duration: Duration, model: &str)->String {
        let steps = ((as_minutes(fc_duration) / 15.0) as u32).to_string();
        odin_data_filename( &bbox.encode(), Some(date), &[ &steps, T::dataset_name()], Some("json"))
    }

    pub fn parse_path<P> (path: P)->Result<Vec<Self>> where P: AsRef<Path> {
        let file = File::open(path)?;
        let br = BufReader::new(file);
        Ok( serde_json::from_reader(br)? )
    }

    pub async fn get_daily_point_archive (config: &OpenMeteoConfig, pos: &GeoPoint, start: &DateTime<Utc>, end: &DateTime<Utc>)->Result<Self> {
        let query = url_query_daily_point::<T>(pos, start, end);
        let url = archive_url(config, &query);

        let body = reqwest::get(url).await?.text().await?;

        Ok ( serde_json::from_str( &body)? )
    }

    pub async fn get_cached_daily_point_archive (config: &OpenMeteoConfig, pos: &GeoPoint, start: &DateTime<Utc>, end: &DateTime<Utc>, path: impl AsRef<Path>)->Result<Self> {
        if path.as_ref().is_file() {
            let data = filepath_contents_as_string(&path)?;
            Ok ( serde_json::from_str( &data)? )

        } else {
            let query = url_query_daily_point::<T>(pos, start, end);
            let url = archive_url(config, &query);
            let body = reqwest::get(url).await?.text().await?;
            std::fs::write( path, &body)?;
            Ok ( serde_json::from_str( &body)? )
        }
    }

    pub async fn get_minutely_bbox_forecasts (config: &OpenMeteoConfig, bbox: &GeoRect, fc_duration: Duration, model: &str)->Result<Vec<Self>> {
        let query = url_query_minutely_bbox::<T>( bbox, fc_duration, model); // static query params are associated to T
        let url = forecast_url(config, &query);
        let body = reqwest::get(url).await?.text().await?;
        Ok ( serde_json::from_str( &body)? )
    }

    pub async fn get_hourly_bbox_forecasts (config: &OpenMeteoConfig, bbox: &GeoRect, days: u32, past_days: u32, model: &str)->Result<Vec<Self>> {
        let query = url_query_hourly_bbox::<T>( bbox, days, past_days, model); // static query params are associated to T
        let url = forecast_url(config, &query);
        let body = reqwest::get(url).await?.text().await?;
        Ok ( serde_json::from_str( &body)? )
    }

    pub async fn get_hourly_point_forecasts (config: &OpenMeteoConfig, pos: &GeoPoint, days: u32, past_days: u32, model: &str)->Result<Self> {
        let query = url_query_hourly_point::<T>( pos, days, past_days, model); // static query params are associated to T
        let url = forecast_url(config, &query);
        let body = reqwest::get(url).await?.text().await?;
        Ok ( serde_json::from_str( &body)? )
    }

    pub async fn get_cached_hourly_point_forecasts (config: &OpenMeteoConfig, pos: &GeoPoint, days: u32, past_days: u32, model: &str, path: impl AsRef<Path>)->Result<Self> {
        if path.as_ref().is_file() {
            let data = filepath_contents_as_string(&path)?;
            Ok ( serde_json::from_str( &data)? )

        } else {
            let query = url_query_hourly_point::<T>( pos, days, past_days, model); // static query params are associated to T
            let url = forecast_url(config, &query);
            let body = reqwest::get(url).await?.text().await?;
            std::fs::write( path, &body)?;
            Ok ( serde_json::from_str( &body)? )
        }
    }

    pub async fn get_hourly_point_list_forecasts (config: &OpenMeteoConfig, pos: &[GeoPoint], days: u32, past_days: u32, model: &str)->Result<Vec<Self>> {
        let query = url_query_hourly_points::<T>( pos, days, past_days, model); // static query params are associated to T
        let url = forecast_url(config, &query);
        let body = reqwest::get(url).await?.text().await?;

        Ok ( serde_json::from_str( &body)? )
    }

    pub fn current_time (&self)->Option<DateTime<Utc>> {
        self.current_data.as_ref().and_then( |cd| cd.current.nth_time_step(0).ok()) // there always is one value
    }

    pub fn minutely_times (&self)->Option<Vec<DateTime<Utc>>> {
        self.minutely_data.as_ref().and_then( |md| md.minutely_15.time_steps().ok())
    }

    pub fn hourly_times (&self)->Option<Vec<DateTime<Utc>>> {
        self.hourly_data.as_ref().and_then( |hd| hd.hourly.time_steps().ok())
    }

    pub fn daily_times (&self)->Option<Vec<DateTime<Utc>>> {
        self.daily_data.as_ref().and_then( |hd| hd.daily.time_steps().ok())
    }

    /// get all positions of a slice of OpenMeteoLocationData values as an vec of GeoPoints
    pub fn geo_points (lds: &[Self])->Vec<GeoPoint> {
        lds.iter().map( |ld| GeoPoint::from_lon_lat_degrees( ld.metadata.longitude as f64, ld.metadata.latitude as f64)).collect()
    }
}

/// factored out part of OpenMeteoLocationData that is independent of OpenMeteoDataSet type
/// (this will get flattened when deserializing)
#[derive(Deserialize,Debug)]
#[public_struct]
struct LocationMetaData {
    latitude: f64,
    longitude: f64,
    generationtime_ms: f64,
    utc_offset_seconds: u32,
    timezone: String,
    timezone_abbreviation: String,
}


//--- those structs are flattened aggregates for optional current/minutely/hourly/daily values

#[derive(Deserialize,Debug)]
#[public_struct]
struct CurrentData<T> {
    current_units: HashMap<String,String>,
    current: T,
}

impl <T> Deref for CurrentData<T> where T: OpenMeteoDataSet {
    type Target = T;
    fn deref(&self) -> &Self::Target { &self.current }
}

#[derive(Deserialize,Debug)]
#[public_struct]
struct DailyData<T> {
    daily_units: HashMap<String,String>,
    daily: T,
}

impl <T> Deref for DailyData<T> where T: OpenMeteoDataSet {
    type Target = T;
    fn deref(&self) -> &Self::Target { &self.daily }
}

#[derive(Deserialize,Debug)]
#[public_struct]
struct HourlyData<T> {
    hourly_units: HashMap<String,String>,
    hourly: T,
}

impl <T> Deref for HourlyData<T> where T: OpenMeteoDataSet {
    type Target = T;
    fn deref(&self) -> &Self::Target { &self.hourly }
}

#[derive(Deserialize,Debug)]
#[public_struct]
struct MinutelyData<T> {
    minutely_15_units: HashMap<String,String>,
    minutely_15: T,
}

impl <T> Deref for MinutelyData<T> where T: OpenMeteoDataSet {
    type Target = T;
    fn deref(&self) -> &Self::Target { &self.minutely_15 }
}


/// turn a set of homogenous OpenMeteoLocationData records into a list of GDAL Datasets with one instance per
/// time point. This essentially creates geographic grids with single time points out of a list of locations that all
/// have the same OpenMeteoDataSet data with the same list of time points
pub fn create_datasets<T,P,U,F> (
    req: &WxDataSetRequest,
    data: &[OpenMeteoLocationData<T>],
    temp_cat: OpenMeteoTemporal, // do we want current, minutely, hourly or daily data
    time_interval: Range<usize>,
    alg: &GdalGridAlgorithmOptions,
    tgt_dir: P,
    file_ext: &str,
    create_opts: &RasterCreationOptions,
    n_tgt_bands: usize, // we might need additional ones for computed values
    x_res: f64,
    y_res: f64,
    mut push_timestep: F
)->Result<Vec<Dataset>>
    where
        T: OpenMeteoDataSet + for <'a> Deserialize<'a>,
        P: AsRef<Path>,
        U: GdalType + NumericType,
        F: FnMut(&OpenMeteoLocationData<T>,usize,&mut Vec<Vec<f64>>),
{
    if data.is_empty() { return Ok( Vec::with_capacity(0)) }
    let ref_data = &data[0];

    //let time_steps = data[0].time_steps(); // they are all the same between data points
    let time_steps = match temp_cat {
        OpenMeteoTemporal::Current => vec![ ref_data.current_time().ok_or( op_failed!("no current time fields"))?],
        OpenMeteoTemporal::Minutely => ref_data.minutely_times().ok_or( op_failed!("no minutely time fields"))?,
        OpenMeteoTemporal::Hourly => ref_data.hourly_times().ok_or( op_failed!("no minutely time fields"))?,
        OpenMeteoTemporal::Daily => ref_data.daily_times().ok_or( op_failed!("no daily time fields"))?,
    };

    if time_steps.is_empty() { return Ok( Vec::with_capacity(0)) }
    let basedate = time_steps[0]; // TODO - is this always true or should we pass this in?

    let driver_name = get_driver_name_for_extension( file_ext).ok_or( op_failed!("unknown file extension"))?;
    let raster_driver = get_driver_by_name(driver_name).ok_or( op_failed!("unsupported raster driver"))?;

    let [west,south,east,north] = req.bbox.to_wsen_degrees();

    let x_size = ((east - west) / x_res) as usize;
    let y_size = ((north - south) / y_res) as usize;

    let x_coords: Vec<f64> = data.iter().map( |d| d.metadata.longitude as f64).collect();
    let y_coords: Vec<f64> = data.iter().map( |d| d.metadata.latitude as f64).collect();

    let n_pos = x_coords.len();

    // the intermediate representation of values
    let mut tgt_vs: Vec<Vec<f64>> = vec![ Vec::with_capacity(n_pos); n_tgt_bands];
    let mut ts = 0;
    let mut result = Vec::new();

    for date in time_steps[time_interval].iter() {
        for v in &mut tgt_vs { v.clear(); }

        for d in data {
            push_timestep( d, ts, &mut tgt_vs); // the passed-in closure that pushes the fields we are interested in
        }

        let step = (*date - basedate).num_hours().to_string();
        //let fname = odin_data_filename( &req.region, Some(basedate), &[ &step, req.model_name.as_str(), req.dataset_name.as_str()], Some(file_ext));
        let fname = odin_data_filename( &req.region, Some(basedate), &[ &step, req.model_name.as_str()], Some(file_ext));
        let path = tgt_dir.as_ref().to_path_buf().join( fname);
        let create_opts = if is_gtiff_driver(&raster_driver) {
            RasterCreationOptions::from_iter( ["COMPRESS=DEFLATE", "PREDICTOR=2"])
        } else {
            RasterCreationOptions::new()
        };

        let mut ds = create_grid_ds::<U,_>(
            &raster_driver, &path,
            4326, west, east, south, north,
            x_size, y_size,
            &x_coords, &y_coords,
            &tgt_vs,
            alg,
            &create_opts
        )?;

        // GDAL automatically sets the filename as the description metainfo
        // we do add the forecast date here
        //ds.set_description( &path);
        ds.set_metadata_item( FORECAST_EPOCH, &date.timestamp().to_string(), "");
        ds.set_metadata_item( FORECAST_STEP, &ts.to_string(), "");

        result.push(ds);
        ts += 1;
    }

    Ok( result )
}

/* #region helper funcs  *********************************************************************************************************/

fn empty_units()->HashMap<String,String> {
    HashMap::with_capacity(0)
}

pub fn utc_from_timespec (ts: &str)->Result<DateTime<Utc>> {
    let nd = NaiveDateTime::parse_from_str( ts, "%Y-%m-%dT%H:%M")?;
    Ok( DateTime::from_naive_utc_and_offset( nd, Utc) )
}

pub fn get_timesteps_from_file<P> (path: P)->Result<Vec<DateTime<Utc>>> where P: AsRef<Path> {
    let data = fs::read(path)?;
    let s = str::from_utf8( &data)?;

    let mut v = Vec::new();
    for ts in TIMES_RE.captures(s).ok_or( op_failed!("invalid file"))?.get(1).ok_or( op_failed!("no times"))?.as_str().split(',') {
        let nd = NaiveDateTime::parse_from_str( ts, "\"%Y-%m-%dT%H:%M\"")?;
        let date = nd.and_utc();
        v.push(date);
    }

    Ok(v)
}

/// get the required number of forecast days (based on current UTC time) to cover the given duration.
/// Note that open-meteo hourly forecasts always start at 00h of the current UTC day. Only if h(utc) + dur > 23h do
/// we have to query > 1 days
pub fn get_fc_days (now: &DateTime<Utc>, dur: Duration)->u32 {
    let h_now = now.hour();
    let n_hours = (dur.as_secs() / 3600) as u32;

    ((h_now + n_hours) / 24) + 1
}

/* #endregion helper funcs */
