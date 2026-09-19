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
    path::{Path,PathBuf},
    io::{Read,BufReader},
    fs::{File},
    collections::HashMap,
    hash::{Hash,DefaultHasher,Hasher},
    ops::{Deref,Range},
    fs
};
use chrono::{DateTime,Utc,Datelike,Timelike,NaiveDate,NaiveTime, NaiveDateTime};
use serde::{Deserialize,Serialize, de::{Deserialize as DeserializeTrait, DeserializeOwned}};
use serde_json;
use gdal::{Metadata, raster::{GdalType, RasterCreationOptions}};
use lazy_static::lazy_static;
use regex::Regex;
use uom::si::length::{meter};
use uom::si::f64::Length;
use ndarray::prelude::*;

use odin_build::define_load_config;
use odin_macro::public_struct;

use odin_common::{
    datetime::{as_days, as_minutes,ymd, days},
    collections::get_cond_range,
    strings::{format_float},
    fs::{ensure_writable_dir, filepath_contents_as_string, odin_data_filename},
    geo::{GeoPoint, GeoRect},
    net::{get_deserialized_json_response,get_deserialized_cached_json_response},
    num_type::{NumericType,average}, sliced,
    utm::{self, UtmRect}
};
use odin_gdal::{
    Dataset as GdalDataset, get_driver_for_extension, is_gtiff_driver, set_band_meta,
    grid::{self as gdal_grid, GdalGridAlgorithmOptions, create_grid_ds_from_scatter}
};
use odin_wx::{WxService,WxDataSetRequest,WxFileAvailable,AddDataSet,RemoveDataSet};

pub mod wx_service;
pub mod dataset;
use dataset::PrecipitationSum;

pub mod queries;
pub mod ids;
use ids::{FieldId,ModelId};

pub mod errors;
use errors::op_failed;

pub mod actor;
pub use actor::OpenMeteoActor;
pub use actor::OpenMeteoActorMsg;

pub mod convert;
pub mod grid;

use crate::errors::OdinOpenMeteoError;
pub type Result<T> = std::result::Result<T,errors::OdinOpenMeteoError>;

lazy_static! {
    static ref TIMES_RE: Regex = Regex::new( r#""time":\[(.*?)\]"#).unwrap();

    static ref CACHE_DIR: PathBuf = {
        let path = odin_build::cache_dir().join("odin_openmeteo");
        ensure_writable_dir(&path).expect( &format!("invalid Open-Meteo cache dir: {path:?}"));
        path
    };
}

define_load_config!{}

pub const MAX_POINTS: usize = 1000; // maximum number of points allowed in one query (see https://open-meteo.com/en/docs/gfs-ap)

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

/*
 * the new type system for Open-Meteo data is centered around the requested time-resolution. While we could theoretically
 * combine several of these in a single query (e.g. current observation and hourly forecasts) this is not the typical
 * use (either minutely or hourly forecasts, daily history or current snapshot) and would make the type system more
 * complicated (different variadic data set types for different time scales), harder/more expensive to use (check for optional timestep data)
 * and less safe (e.g. operations such as IDW that only work for homogenous data set types we can't check at compile time).
 * By using time-resolution based queries we can test once (upfront) if we got the requested data instead of checking on every access
 */

/// the base trait for Open-Meteo data sets
pub trait DataSet {
    // we prefer the query strings over a list of FieldIds since query params might differ. Used only to construct request URLs
    fn fields ()->&'static str;
    fn units ()->&'static str {
        ""
        //"&temperature_unit=celsius&windspeed_unit=ms&precipitation_unit=mm"
    }
    fn dataset_name ()->&'static str;

    // methods that can be used to construct new or modify existing dataset instances from spatial relatives.
    // We need those since OpenMeteo is a multi-model database and we cannot rely on regular grid data to be returned
    // (even if we ask for specific grid positions as location lists)

    /// inverse distance weighted mutation. Each element pair consists of the normalized weight and the corresponding dataset item
    /// own fields are computed by summation of the pair products: sum(ds[i].0 * ds[i].1)
    /// Note this requires a neutral (zero'ed) ctor
    fn idw (&mut self, ds: &[(f64,&Self)]);

    /// ctor to interpolate linearly between a and b with ratio d
    fn interpolate (a: &Self, b: &Self, d: f64)->Self;

    /// incremental average mutation
    fn average (&mut self, next: &Self, n: usize);

    /// drop all time points outside given interval. This modifies the receiver
    fn filter_time_steps (&mut self, start: &DateTime<Utc>, end: &DateTime<Utc>);
}

/// a DataSet that contains data for a list of time steps. Used for forecasts and historical queries
pub trait VecDataSet : DataSet {
    fn times(&self)->&[DateTime<Utc>];

    /// constructor for instance that is zero except of time field (seed for IDW accumulation)
    fn new (times: Vec<DateTime<Utc>>)->Self;

    fn n_time_steps (&self)->usize {
        self.times().len()
    }

    fn nth_time_step (&self, n: usize)->Result<DateTime<Utc>> {
        Ok( self.times().get(n).ok_or( op_failed!("no such timestep"))?.clone() )
    }

    fn time_step_index (&self, date: &DateTime<Utc>)->Option<usize> {
        self.times().iter().position(|d| d == date)
    }
}

/// an DataSet that only has data for one time point. Used for current queries
pub trait ScalarDataSet : DataSet {
    fn time (&self)->DateTime<Utc>;
    fn new (time: DateTime<Utc>)->Self;
}

//--- the types we deserialize from time-resolution specific query results

/// result data for 15min queries
#[derive(Deserialize,Debug)]
#[public_struct]
struct MinutelyLocationData<T> where T: VecDataSet {
    #[serde(flatten)]
    meta_data: LocationMetaData,

    minutely_15_units: HashMap<String,String>,
    minutely_15: T
}

impl<T> LocationData<T> for MinutelyLocationData<T> where T: VecDataSet {
    fn meta_data (&self)->&LocationMetaData { &self.meta_data }
    fn fields_type ()->&'static str { "minutely_15" }
    fn time_steps (&self)->Vec<DateTime<Utc>> { self.minutely_15.times().to_vec() }
}

/// result data for hourly queries
#[derive(Deserialize,Debug)]
#[public_struct]
struct HourlyLocationData<T> where T: VecDataSet {
    #[serde(flatten)]
    meta_data: LocationMetaData,

    hourly_units: HashMap<String,String>,
    hourly: T
}

impl<T> LocationData<T> for HourlyLocationData<T> where T: VecDataSet  {
    fn meta_data (&self)->&LocationMetaData { &self.meta_data }
    fn fields_type ()->&'static str { "hourly" }
    fn time_steps (&self)->Vec<DateTime<Utc>> { self.hourly.times().to_vec() }
}

/// result data for daily queries
#[derive(Deserialize,Debug)]
#[public_struct]
struct DailyLocationData<T> where T: VecDataSet {
    #[serde(flatten)]
    meta_data: LocationMetaData,

    daily_units: HashMap<String,String>,
    daily: T
}

impl<T> LocationData<T> for DailyLocationData<T> where T: VecDataSet {
    fn meta_data (&self)->&LocationMetaData { &self.meta_data }
    fn fields_type ()->&'static str { "daily" }
    fn time_steps (&self)->Vec<DateTime<Utc>> { self.daily.times().to_vec() }
}

/// result data for current time queries
#[derive(Deserialize,Debug)]
#[public_struct]
struct CurrentLocationData<T> where T: ScalarDataSet {
    #[serde(flatten)]
    meta_data: LocationMetaData,

    current_units: HashMap<String,String>,
    current: T
}

impl<T> LocationData<T> for CurrentLocationData<T> where T: ScalarDataSet {
    fn meta_data (&self)->&LocationMetaData { &self.meta_data }
    fn fields_type ()->&'static str { "current" }
    fn time_steps (&self)->Vec<DateTime<Utc>> { vec![self.current.time()] }
}

//--- the low level explicit parameter URL ctors

pub fn get_bbox_query (
    model: &str,
    units: &str,
    field_type: &str, fields: &str,
    start: &DateTime<Utc>, end: &DateTime<Utc>,
    bbox: &GeoRect
) -> String {
    format!("models={}{}&{}={}&start_date={}&end_date={}&bounding_box={}",
        model, units,
        field_type, fields,
        ymd(start), ymd(end),
        format_bbox( bbox)
    )
}

fn format_bbox (geo_rect: &GeoRect)->String {
    format!( "{},{},{},{}",
        format_float( geo_rect.south().degrees(), 4),
        format_float( geo_rect.west().degrees(), 4),
        format_float( geo_rect.north().degrees(), 4),
        format_float( geo_rect.east().degrees(), 4)
    )
}

pub fn get_points_query (
    model: &str,
    units: &str,
    field_type: &str, fields: &str,
    start: &DateTime<Utc>, end: &DateTime<Utc>,
    pts: &[GeoPoint]
) -> String {
    format!("models={}{}&{}={}&start_date={}&end_date={}&&latitude={}&longitude={}",
        model, units,
        field_type, fields,
        ymd(start), ymd(end),
        format_latitudes(pts), format_longitudes(pts)
    )
}

fn format_latitudes (pts: &[GeoPoint])->String {
    pts.iter().map( |p| format_float( p.latitude_degrees(), 4)).collect::<Vec<String>>().join(",")
}

fn format_longitudes (pts: &[GeoPoint])->String {
    pts.iter().map( |p| format_float( p.longitude_degrees(), 4)).collect::<Vec<String>>().join(",")
}

pub fn get_point_query (
    model: &str,
    units: &str,
    field_type: &str, fields: &str,
    start: &DateTime<Utc>, end: &DateTime<Utc>,
    p: &GeoPoint
) -> String {
    format!("models={}{}&{}={}&start_date={}&end_date={}&&latitude={}&longitude={}",
        model, units,
        field_type, fields,
        ymd(start), ymd(end),
        format_float( p.latitude_degrees(), 4), format_float( p.longitude_degrees(), 4)
    )
}

pub fn get_forecast_url (config: &OpenMeteoConfig, query: &str)->String {
    get_url( &config.forecast_url, query)
}

pub fn get_archive_url (config: &OpenMeteoConfig, query: &str)->String {
    get_url( &config.archive_url, query)
}

pub fn get_url (base_url: &str, query: &str)->String {
    format!("{}?{}", base_url, query)
}

pub fn get_meta_url (base_url: &str, model_name: &str)->String {
    format!("{}/{}/static/meta.json", base_url, model_name)
}

/// common interface for all location data
pub trait LocationData<T> where T: DataSet, Self: Sized {

    fn meta_data (&self)->&LocationMetaData;
    fn fields_type ()->&'static str;
    fn time_steps (&self)->Vec<DateTime<Utc>>;

    //--- query construction

    fn get_bbox_query (model: ModelId, start: &DateTime<Utc>, end: &DateTime<Utc>, bbox: &GeoRect)->String {
        get_bbox_query( model.into(), T::units(), Self::fields_type(), T::fields(), start, end, bbox)
    }

    fn get_points_query (model: ModelId, start: &DateTime<Utc>, end: &DateTime<Utc>, pts: &[GeoPoint])->String {
        get_points_query( model.into(), T::units(), Self::fields_type(), T::fields(), start, end, pts)
    }

    fn get_point_query (model: ModelId, start: &DateTime<Utc>, end: &DateTime<Utc>, pt: &GeoPoint)->String {
        get_point_query( model.into(), T::units(), Self::fields_type(), T::fields(), start, end, pt)
    }

    //--- misc helpers

    /// get all positions of a slice of LocationData instances as an vec of GeoPoints
    fn geo_points (lds: &[Self])->Vec<GeoPoint> {
        lds.iter().map( |ld| {
            let meta_data = ld.meta_data();
            GeoPoint::from_lon_lat_degrees( meta_data.longitude as f64, meta_data.latitude as f64)
        }).collect()
    }
}

pub fn parse_location_data<T,L,P> (path: P)->Result<Vec<L>>
    where P: AsRef<Path>, T: DataSet + DeserializeOwned, L: LocationData<T> + DeserializeOwned
{
    let file = File::open(path)?;
    let br = BufReader::new(file);
    Ok( serde_json::from_reader(br)? )
}

pub async fn get_json_response<T> (url: &str)->Result<Vec<T>> where T: Sized + for <'a> Deserialize<'a> {
    Ok( get_deserialized_json_response(url).await? )
}

pub async fn get_cached_json_response<T,P> (url: &str, path: P)->Result<Vec<T>> where T: Sized + for <'a> Deserialize<'a>, P: AsRef<Path> {
    Ok( get_deserialized_cached_json_response(url, path).await? )
}


/// factored out part of LocationData that is independent of timestep resolution
/// (this is flattened in deserialization input)
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

impl LocationMetaData {
    pub fn geo_pos(&self)->GeoPoint {
        GeoPoint::from_lon_lat_degrees( self.longitude, self.latitude)
    }
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

/// 0-based index for hourly observations starting at current time
/// open-meteo returns hourly data as full UTC days
pub fn start_step (now: &DateTime<Utc>)->usize {
    now.hour() as usize
}

pub fn get_time_step_range<T: VecDataSet> (arr: &Array2<Option<T>>, start: &DateTime<Utc>, n_steps: usize) -> Result<Range<usize>> {
    let times = get_time_steps( arr)?;
    let i0 = get_start_step( start, times)?;
    let i1 = (i0 + n_steps).min( times.len());

    Ok( i0..i1 )
}

/// get the times slice of the first set grid cell value (all cells are supposed to have the same time value)
fn get_time_steps<T: VecDataSet> (arr: &Array2<Option<T>>)->Result<&[DateTime<Utc>]> {
    if let Some(o) = arr.iter().find( |p| p.is_some()) {
        Ok( o.as_ref().unwrap().times() )
    } else {
        Err( op_failed!("no grid data"))
    }
}

fn get_start_step (start: &DateTime<Utc>, times: &[DateTime<Utc>])->Result<usize> {
    let mut i: usize = 0;
    let len = times.len();

    while i < len {
        if times[i] == *start {
            return Ok(i)
        }
        if times[i] > *start {
            if i > 0 { return Ok(i-1) } else { return Err( op_failed!("start not covered") ) }
        }
        i += 1;
    }

    Err( op_failed!("start not covered") )
}


/* #endregion helper funcs */
