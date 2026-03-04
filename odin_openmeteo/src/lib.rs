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
    time::Duration, fmt::Debug, sync::Arc, any::type_name,
    path::{Path,PathBuf}, ops::Range,
    io::{Read,BufReader}, fs::{File},
    collections::HashMap,
    hash::{Hash,DefaultHasher,Hasher},
    fs
};
use chrono::{DateTime,Utc,Datelike,Timelike,NaiveDate,NaiveTime, NaiveDateTime};
use serde::{Deserialize,Serialize, de::{Deserialize as DeserializeTrait}};
use serde_json;
use gdal::{Metadata, raster::GdalType};
use lazy_static::lazy_static;
use regex::Regex;

use odin_build::define_load_config;
use odin_macro::public_struct;
use odin_actor::{ActorHandle};
use odin_common::{fs::{odin_data_filename,ensure_writable_dir}, geo::{GeoPoint, GeoRect}, num_type::NumericType, sliced, utm, datetime::as_days};
use odin_gdal::{Dataset, get_driver_by_name, get_driver_name_for_extension, grid::{self, GdalGridAlgorithmOptions, create_grid_ds}, set_band_meta};
use odin_wx::{WxService,WxDataSetRequest,WxFileAvailable,AddDataSet,RemoveDataSet};

pub mod fields;
use fields::{FieldId,ModelId};

pub mod errors;
use errors::op_failed;

pub mod actor;
pub use actor::OpenMeteoActor;
pub use actor::OpenMeteoActorMsg;

pub mod convert;

use crate::errors::OdinOpenMetError;
pub type Result<T> = std::result::Result<T,errors::OdinOpenMetError>;

/// the Dataset metainfo key for the forecast time
pub const FORECAST_EPOCH: &'static str = "forecast_epoch";

/// the Dataset metainfo key for the forecast time step (usually number of hours since base time)
pub const FORECAST_STEP: &'static str = "forecast_step";

lazy_static! {
    static ref CACHE_DIR: PathBuf = {
        let path = odin_build::cache_dir().join("odin_openmeteo");
        ensure_writable_dir(&path).expect( &format!("invalid Open-Meteo cache dir: {path:?}"));
        path
    };
}

define_load_config!{}

#[derive(Deserialize,Debug)]
pub struct OpenMeteoConfig {
    pub data_url: String, // the model independent invariant data url part
    pub meta_url: String, // the model independent invariant meta url part

    pub initial_delay: Duration, // to apply on top of the computed schedule
    pub retry_delay: Duration, // between retries of not-yet-available downloads

    /// how long to keep downloaded HRRR files
    pub max_age: Duration,
}

pub fn data_url_query (bbox: &GeoRect, fc_duration: Duration, model: &str, fields_query: &str)->String {
    let fc_days = (as_days( fc_duration).round() as u32).max(2);
    format!("models={}&bounding_box={},{},{},{}&forecast_days={}&cell_selection=nearest&wind_speed_unit=ms&hourly={}",
        model,
        bbox.south().degrees(), bbox.west().degrees(), bbox.north().degrees(), bbox.east().degrees(),
        fc_days,
        fields_query
    )
}

pub fn data_url (config: &OpenMeteoConfig, query: &str)->String {
    format!("{}?{}", config.data_url, query)
}

pub fn meta_url (config: &OpenMeteoConfig, model_name: &str)->String {
    format!("{}/{}/static/meta.json", config.meta_url, model_name)
}

pub struct OpenMeteoService {
    wx_name: Arc<String>,
    model_name: Arc<String>,
    dataset_name: Arc<String>,

    fields: Vec<FieldId>,
    fields_query: String, // the invariant non-region part of the query (computed from fields and levels)
    hself: ActorHandle<OpenMeteoActorMsg>
}

impl OpenMeteoService {
    pub fn new (model: ModelId, dataset_name: Arc<String>, fields: Vec<FieldId>, hself: ActorHandle<OpenMeteoActorMsg>)->Self {
        let wx_name = Arc::new( type_name::<Self>().to_string());
        let model_name = Arc::new( model.as_ref().to_string());
        let fields_query = FieldId::as_list_string(&fields);

        OpenMeteoService{wx_name, model_name, dataset_name, fields, fields_query, hself}
    }

    pub fn new_basic_ifs (hself: ActorHandle<OpenMeteoActorMsg>)->Self {
        let dataset_name = Arc::new( "basic".to_string());
        let fields = BasicEcmwfIfsData::hourly_fields();

        Self::new( ModelId::ecmwf_ifs, dataset_name, fields, hself)
    }
}

impl WxService for OpenMeteoService {
    fn wx_name(&self) -> Arc<String> {
        self.wx_name.clone()
    }

    fn model_name (&self)->Arc<String> {
        self.model_name.clone()
    }

    fn dataset_name(&self)->Arc<String> {
        self.dataset_name.clone()
    }

    fn try_send_add_dataset(&self,req: Arc<WxDataSetRequest>) -> odin_actor::Result<()> {
        self.hself.try_send_msg( AddDataSet(req))
    }

    fn try_send_remove_dataset(&self,req: Arc<WxDataSetRequest>) -> odin_actor::Result<()> {
        self.hself.try_send_msg( RemoveDataSet(req))
    }

    fn create_request (&self, region: Arc<String>, bbox: GeoRect, fc_duration: Duration)->WxDataSetRequest {
        let fc_days = as_days( fc_duration).round() as u32;
        let query = data_url_query(&bbox, fc_duration, self.model_name.as_str(), self.fields_query.as_str());
        let wx_name = self.wx_name.clone();
        let model_name = self.model_name.clone();
        let dataset_name = self.dataset_name.clone();
        WxDataSetRequest { region, bbox, wx_name, model_name, dataset_name, fc_duration, query }
    }

    fn to_wx_grids (&self, fa: &WxFileAvailable)->odin_wx::Result<Vec<Arc<PathBuf>>> {
        convert::basic_ecmwf_ifs_to_hrrr( &fa.request.as_ref(), fa.path.as_ref(), CACHE_DIR.as_path()).map_err(|e|
            odin_wx::errors::OdinWxError::OpFailedError(format!("cannot convert to HRRR compliant grids: {}", e)))
    }
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

/// the structure we parse data query responses into
/// this is normally used by clients of odin_openmet
/// Note that we get responses as arrays of OpenMetGridPoint data
/// Note also that OpenMetGridPoints are on a Gaussian grid (i.e. not a regular lon/lat grid) and contain
/// the different forecast hours for each field, i.e. we have to use Odin_gdal::grid to populate regular grids
/// from forecast-time slices over a collection of OpenMetGridPoint values
#[derive(Deserialize,Debug)]
#[public_struct]
struct OpenMeteoLocationData<T> where T: OpenMeteoData {
    latitude: f32,
    longitude: f32,
    generationtime_ms: f64,
    utc_offset_seconds: u32,
    timezone: String,
    timezone_abbreviation: String,
    hourly_units: HashMap<String,String>,
    hourly: T
}

impl <T> OpenMeteoLocationData<T> where T: OpenMeteoData + for <'a> Deserialize<'a> {
    pub fn time_steps (&self)->Vec<DateTime<Utc>> {
        self.hourly.time_steps()
    }

    pub fn n_time_steps (&self)->usize {
        self.hourly.n_time_steps()
    }

    pub fn parse_reader<R> (rdr: R)->Result<Vec<Self>> where R: Read {
        Ok( serde_json::from_reader(rdr)? )
    }

    pub fn parse_str (s: &str)->Result<Vec<Self>> {
        Ok( serde_json::from_str( s)? )
    }

    pub fn parse_path<P> (path: P)->Result<Vec<Self>> where P: AsRef<Path> {
        let file = File::open(path)?;
        let br = BufReader::new(file);
        Self::parse_reader( br)
    }

    pub fn create_datasets<P,U,F> (
        req: &WxDataSetRequest,
        data: &[Self],
        time_interval: Range<usize>,
        alg: &GdalGridAlgorithmOptions,
        tgt_dir: P,
        file_ext: &str,
        n_tgt_bands: usize,
        mut push_timestep: F
    )->Result<Vec<Dataset>>
        where
            P: AsRef<Path>,
            U: GdalType + NumericType,
            F: FnMut(&Self,usize,&mut Vec<Vec<f64>>),
    {
        if data.is_empty() { return Ok( Vec::with_capacity(0)) }

        let time_steps = data[0].time_steps(); // they are all the same between data points

        if time_steps.is_empty() { return Ok( Vec::with_capacity(0)) }
        let basedate = time_steps[0]; // TODO - is this always true or should we pass this in?

        let driver_name = get_driver_name_for_extension( file_ext).ok_or( op_failed!("unknown file extension"))?;
        let raster_driver = get_driver_by_name(driver_name).ok_or( op_failed!("unsupported raster driver"))?;

        let [west,south,east,north] = req.bbox.to_wsen_degrees();
        let (x_res,y_res) = T::resolution();

        let x_size = ((east - west) / x_res) as usize;
        let y_size = ((north - south) / y_res) as usize;

        let x_coords: Vec<f64> = data.iter().map( |d| d.longitude as f64).collect();
        let y_coords: Vec<f64> = data.iter().map( |d| d.latitude as f64).collect();

        let n_pos = x_coords.len();
        let n_fields = T::n_hourly_fields();

        // the intermediate representation of values
        let mut tgt_vs: Vec<Vec<f64>> = vec![ Vec::with_capacity(n_pos); n_tgt_bands];
        let mut ts = 0;
        let mut result = Vec::new();

        for date in time_steps[time_interval].iter() {
            for v in &mut tgt_vs { v.clear(); }

            for d in data {
                push_timestep( d, ts, &mut tgt_vs);
            }

            let step = (*date - basedate).num_hours().to_string();
            //let fname = odin_data_filename( &req.region, Some(basedate), &[ &step, req.model_name.as_str(), req.dataset_name.as_str()], Some(file_ext));
            let fname = odin_data_filename( &req.region, Some(basedate), &[ &step, req.model_name.as_str()], Some(file_ext));
            let path = tgt_dir.as_ref().to_path_buf().join( fname);
            let mut ds = create_grid_ds::<U,_>(
                &raster_driver, &path,
                4326, west, east, south, north,
                x_size, y_size,
                &x_coords, &y_coords,
                &tgt_vs,
                alg
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
}

/// the common interface for specific 'hourly' data sets
pub trait OpenMeteoData  {
    fn dataset_name ()->&'static str;
    fn time_steps (&self)->Vec<DateTime<Utc>>;
    fn n_time_steps (&self)->usize;
    fn hourly_fields ()->Vec<FieldId>; // NOTE - those come from respective open-meteo APIs, we can't chose them freely
    fn n_hourly_fields ()->usize;
    fn resolution ()->(f64,f64);
    fn push_timestep (&self, timestep: usize, v: &mut Vec<Vec<f64>>);
}


pub type BasicEcmwfIfs = OpenMeteoLocationData<BasicEcmwfIfsData>;

#[derive(Deserialize,Debug)]
#[public_struct]
struct BasicEcmwfIfsData {
    time: Vec<String>,
    temperature_2m: Vec<f32>,
    surface_pressure: Vec<f32>,
    relative_humidity_2m: Vec<u8>,
    cloud_cover: Vec<u8>,
    wind_speed_10m: Vec<f32>,
    wind_direction_10m: Vec<u16>,
    wind_speed_100m: Vec<f32>,
    wind_direction_100m: Vec<u16>,
}

impl OpenMeteoData for BasicEcmwfIfsData {
    fn dataset_name()->&'static str {
        "basic"
    }

    fn time_steps (&self)->Vec<DateTime<Utc>> {
        self.time.iter().map( |s| {
            let nd = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").unwrap();
            DateTime::from_naive_utc_and_offset( nd, Utc)
        }).collect()
    }

    fn n_time_steps (&self)->usize {
        self.time.len()
    }

    /// note these are from open-meteo query parameters
    fn hourly_fields ()->Vec<FieldId> {
        use FieldId::*;
        vec![
            temperature_2m,
            surface_pressure,
            relative_humidity_2m,
            cloud_cover,
            wind_speed_10m,
            wind_direction_10m,
            wind_speed_100m,
            wind_direction_100m,
        ]
    }

    fn n_hourly_fields ()->usize {
        8
    }

    fn resolution ()->(f64,f64) {
        (0.1, 0.1) // in degrees
    }

    fn push_timestep (&self, timestep: usize, v: &mut Vec<Vec<f64>>) {
        v[0].push( self.temperature_2m[timestep].into());
        v[1].push( self.surface_pressure[timestep].into());
        v[2].push( self.relative_humidity_2m[timestep].into());
        v[3].push( self.cloud_cover[timestep].into());
        v[4].push( self.wind_speed_10m[timestep].into());
        v[5].push( self.wind_direction_10m[timestep].into());
        v[6].push( self.wind_speed_100m[timestep].into());
        v[7].push( self.wind_direction_100m[timestep].into());
    }
}

/// default map function for create_datasets(..). This just copies fields 1:1 as f64 values
pub fn push_timestep<T> (src: &OpenMeteoLocationData<T>, timestep: usize, tgt: &mut Vec<Vec<f64>>)
where for <'a>
    T: OpenMeteoData + Deserialize<'a>
{
    src.hourly.push_timestep( timestep, tgt);
}

lazy_static! {
    static ref TIMES_RE: Regex = Regex::new( r#""time":\[(.*?)\]"#).unwrap();
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
