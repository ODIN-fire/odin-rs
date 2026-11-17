/*
 * Copyright 2026 Peter Mehlitz
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *    http://apache.org
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

//! import and display of NEXRAD WSR-88D Level II data (see <https://registry.opendata.aws/noaa-nexrad/>).
//!
//! Level II volumes are distributed per radar station through the public AWS S3 bucket `unidata-nexrad-level2`
//! as `<YYYY>/<MM>/<DD>/<station>/<station><YYYYMMDD>_<HHMMSS>_V06`, where the time is the scan start. We retrieve the
//! volumes of configured stations, decode them with the `nexrad` crate and compute geo-referenced (EPSG:4326)
//! products such as base and composite reflectivity, which are stored as GeoTIFFs and served as WMS layers.
//!
//! Stations scan with different, weather dependent intervals (volume coverage patterns), i.e. update times have to
//! be estimated from the S3 listings of each station (see [`VolumeTiming`]).
//!
//! This module contains the config, the data model and the S3 access functions

#![allow(unused)]

use std::{path::{Path,PathBuf}, sync::Arc, time::Duration};
use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use lazy_static::lazy_static;

use odin_build::{pkg_cache_dir, pkg_data_dir, define_load_config, define_load_asset};
use odin_common::{datetime::deserialize_duration, fs::EnvPathBuf, s3::{self, S3Client, S3Object}};
use odin_gdal::wms_srv::WmsConfig;

pub mod errors;
use errors::{Result, op_failed};

pub mod product;
pub mod store;
pub mod actor;
pub mod service;

lazy_static! {
    pub static ref CACHE_DIR: PathBuf = { pkg_cache_dir!() };
    pub static ref DATA_DIR: PathBuf = { pkg_data_dir!() };
}

define_load_config!{}
define_load_asset!{}

/// the public S3 bucket with NEXRAD Level II archive volumes
pub const DEFAULT_BUCKET: &str = "unidata-nexrad-level2";
pub const DEFAULT_REGION: &str = "us-east-1";

/* #region config *****************************************************************************************************/

/// configuration for NEXRAD Level II import and display
#[derive(Debug,Clone,Deserialize)]
pub struct NexradConfig {
    /// S3 bucket of Level II archive volumes
    #[serde(default="default_bucket")]
    pub bucket: String,

    /// AWS region of the bucket
    #[serde(default="default_region")]
    pub region: String,

    /// ICAO ids of the stations we import (e.g. "KMUX")
    pub stations: Vec<String>,

    /// the products we compute from each volume
    #[serde(default="default_products")]
    pub products: Vec<NexradProduct>,

    /// cell size of the geographic (EPSG:4326) product grids in meters (default 250m, the Level II gate interval).
    /// This is converted into per-station degree cell sizes that are square at the station latitude. The grid extent of
    /// each station is computed from the max range of its sweeps. Since every gate contributes to the cell that contains
    /// it this does not lose max values for cells that are larger than gates - it is a trade-off between display detail
    /// and file size / computation time
    #[serde(default="default_resolution_m")]
    pub resolution_m: f64,

    /// reflectivity values below this threshold are not displayed (transparent)
    #[serde(default="default_min_dbz")]
    pub min_dbz: f32,

    /// max number of time steps we keep per station and product
    pub max_downloads: usize,

    /// number of the most recent volumes we retrieve per station when starting (if we don't have them yet)
    #[serde(default)]
    pub backfill: usize,

    /// added to the expected availability time of the next volume (last scan + interval + publication delay)
    #[serde(deserialize_with="deserialize_duration")]
    pub check_delay: Duration,

    /// retry interval if the next volume is not available yet at the expected time
    #[serde(deserialize_with="deserialize_duration")]
    pub retry_delay: Duration,

    /// keep downloaded Level II volumes in the cache (e.g. for analysis of polar data). Otherwise volumes are
    /// discarded after the products have been computed
    #[serde(default)]
    pub keep_volumes: bool,

    /// optional CSV file with station positions for stations that are not in the `nexrad` crate registry
    /// (format: `index,X,Y,OBJECTID,siteID,siteName,radarType,antennaElevation`, X/Y are lon/lat)
    #[serde(default)]
    pub stations_csv: Option<EnvPathBuf>,

    /// static WMS settings (title, cache, render limits) - layers are added dynamically for each product time step
    pub wms: WmsConfig,
}

fn default_min_dbz()->f32 { 5.0 }
fn default_resolution_m()->f64 { 250.0 }
fn default_bucket()->String { DEFAULT_BUCKET.to_string() }
fn default_region()->String { DEFAULT_REGION.to_string() }
fn default_products()->Vec<NexradProduct> { vec![ NexradProduct::BaseReflectivity, NexradProduct::CompositeReflectivity ] }

/// products computed from Level II volumes
#[derive(Debug,Clone,Copy,PartialEq,Eq,Hash,Serialize,Deserialize)]
pub enum NexradProduct {
    /// reflectivity of the lowest elevation sweep
    BaseReflectivity,
    /// max reflectivity over all elevation sweeps
    CompositeReflectivity,
}

impl NexradProduct {
    pub const ALL: [NexradProduct;2] = [ NexradProduct::BaseReflectivity, NexradProduct::CompositeReflectivity ];

    /// short name used in file and layer names
    pub fn name (&self)->&'static str {
        match self { NexradProduct::BaseReflectivity => "bref", NexradProduct::CompositeReflectivity => "cref" }
    }

    pub fn title (&self)->&'static str {
        match self { NexradProduct::BaseReflectivity => "Base Reflectivity", NexradProduct::CompositeReflectivity => "Composite Reflectivity" }
    }

    pub fn from_name (name: &str)->Option<Self> {
        Self::ALL.into_iter().find( |p| p.name() == name)
    }
}

/* #endregion config */

/* #region reflectivity encoding **************************************************************************************/

// product GeoTIFFs store reflectivity as 8 bit codes with 0.5 dBZ resolution, using the same encoding as Level II
// reflectivity data (WSR-88D ICD: scale 2.0, offset 66.0), i.e. this does not lose any precision of the source data
// (in radar speak a "gate" is the time window in which to process the returned signal, which effectively is defining
// the max supported range)

/// code for gates with signal below the detection threshold (rendered transparent)
pub const REF_BELOW_THRESHOLD: u8 = 0;
/// code for range folded gates (rendered transparent)
pub const REF_RANGE_FOLDED: u8 = 1;
/// smallest/largest encodable reflectivity
pub const MIN_DBZ: f32 = -32.0;
pub const MAX_DBZ: f32 = 94.5;

/// 8 bit code for a valid reflectivity value (clamped to [MIN_DBZ,MAX_DBZ])
pub fn dbz_to_code (dbz: f32)->u8 {
    (dbz * 2.0 + 66.0).round().clamp( 2.0, 255.0) as u8
}

/// reflectivity for a code, None for below threshold or range folded
pub fn code_to_dbz (code: u8)->Option<f32> {
    if code < 2 { None } else { Some( (code as f32 - 66.0) / 2.0) }
}

/* #endregion reflectivity encoding */

/* #region stations ***************************************************************************************************/

/// a radar station
#[derive(Debug,Clone,PartialEq,Serialize)]
pub struct Station {
    pub id: String,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub elevation_m: Option<f64>,
}

/// get station info, either from the `nexrad` crate site registry or from the given CSV records
pub fn get_station (id: &str, csv_stations: &[Station])->Option<Station> {
    if let Some(e) = nexrad::model::meta::registry::site_by_id( id) {
        let name = match e.state { Some(state) => format!("{}, {}", e.city, state), None => e.city.to_string() };
        Some( Station { id: e.id.to_string(), name, latitude: e.latitude, longitude: e.longitude, elevation_m: Some( e.site_elevation_meters as f64) })
    } else {
        csv_stations.iter().find( |s| s.id == id).cloned()
    }
}

/// read station records from a CSV file with `index,X,Y,OBJECTID,siteID,siteName,radarType,antennaElevation` columns
/// (e.g. `$ODIN_ROOT/data/odin_nexrad/Weather_Radar_Stations.csv`)
pub fn read_stations_csv (path: &Path)->Result<Vec<Station>> {
    let mut rdr = csv::Reader::from_path( path)?;
    let mut stations = Vec::new();
    for rec in rdr.records() {
        let rec = rec?;
        let field = |i: usize| rec.get(i).unwrap_or("").trim();
        let (Ok(lon), Ok(lat)) = (field(1).parse::<f64>(), field(2).parse::<f64>()) else { continue };
        stations.push( Station {
            id: field(4).to_string(),
            name: field(5).to_string(),
            latitude: lat,
            longitude: lon,
            elevation_m: field(7).parse::<f64>().ok(),
        });
    }
    Ok(stations)
}

/// station infos for all configured stations (stations without known position are reported and skipped)
pub fn configured_stations (config: &NexradConfig)->Vec<Station> {
    let csv_stations: Vec<Station> = match &config.stations_csv {
        Some(path) => read_stations_csv( &path.0).unwrap_or_else( |e| { eprintln!("failed to read station CSV {:?}: {e}", path.0); Vec::new() }),
        None => Vec::new()
    };
    config.stations.iter().filter_map( |id| {
        let s = get_station( id, &csv_stations);
        if s.is_none() { eprintln!("unknown NEXRAD station {id} - ignored") }
        s
    }).collect()
}

/* #endregion stations */

/* #region volumes ****************************************************************************************************/

/// a Level II volume in the S3 bucket (representing the objects we download)
#[derive(Debug,Clone,PartialEq)]
pub struct VolumeInfo {
    pub station: String,
    pub scan_time: DateTime<Utc>,             // scan start (from the object name) - unique per station
    pub key: String,                          // S3 object key
    pub size: u64,                            // bytes
    pub last_modified: Option<DateTime<Utc>>, // S3 publication time
}

impl VolumeInfo {
    /// None if the object is not a Level II volume (e.g. a `_MDM` model data file)
    pub fn from_s3_object (o: &S3Object)->Option<Self> {
        let (station, scan_time) = parse_volume_name( o.key_name())?;
        Some( VolumeInfo { station, scan_time, key: o.key_str().to_string(), size: o.size_bytes(), last_modified: o.last_modified_utc() })
    }

    /// object name (last element of key)
    pub fn name (&self)->&str {
        self.key.rsplit('/').next().unwrap_or( &self.key)
    }

    /// time between scan start and publication in the bucket
    pub fn publication_delay (&self)->Option<Duration> {
        self.last_modified.and_then( |lm| (lm - self.scan_time).to_std().ok())
    }
}

/// parse a Level II volume name such as `KMUX20261117_001157_V06` (also `.gz` compressed or older `_V03` volumes)
/// into station and scan start time. Returns None for other objects (e.g. `KMUX20261116_235440_V06_MDM` model data)
pub fn parse_volume_name (name: &str)->Option<(String,DateTime<Utc>)> {
    let name = name.strip_suffix(".gz").unwrap_or( name);
    if name.len() < 19 || !name.is_char_boundary(4) || !name.is_char_boundary(19) { return None }

    let (station, rest) = name.split_at(4);
    if !station.chars().all( |c| c.is_ascii_alphanumeric()) { return None }

    let (dt, suffix) = rest.split_at(15); // YYYYMMDD_HHMMSS
    if !(suffix.is_empty() || (suffix.starts_with("_V") && suffix[2..].chars().all( |c| c.is_ascii_digit()))) { return None }

    let scan_time = NaiveDateTime::parse_from_str( dt, "%Y%m%d_%H%M%S").ok()?.and_utc();
    Some( (station.to_string(), scan_time))
}

/// S3 key prefix of all volumes of a station for a given (UTC) date
pub fn day_prefix (station: &str, date: NaiveDate)->String {
    format!("{}/{}/", date.format("%Y/%m/%d"), station)
}

pub async fn create_s3_client (config: &NexradConfig)->Result<S3Client> {
    Ok( s3::create_s3_client( &config.region).await? )
}

/// volumes of a station for a given date (sorted by scan time), optionally only after a given object key
pub async fn list_volumes (client: &S3Client, bucket: &str, station: &str, date: NaiveDate, start_after: Option<&str>)->Result<Vec<VolumeInfo>> {
    let objects = s3::list_s3_objects( client, bucket, &day_prefix( station, date), start_after).await?;
    let mut volumes: Vec<VolumeInfo> = objects.iter().filter_map( VolumeInfo::from_s3_object).collect();
    volumes.sort_by_key( |v| v.scan_time);
    Ok(volumes)
}

/// volumes of a station with scan times in [since, until] (sorted by scan time). This lists all dates of the
/// interval, i.e. it handles UTC day boundaries
pub async fn list_volumes_between (client: &S3Client, bucket: &str, station: &str, since: DateTime<Utc>, until: DateTime<Utc>)->Result<Vec<VolumeInfo>> {
    let mut volumes = Vec::new();
    let mut date = since.date_naive();
    while date <= until.date_naive() {
        let vs = list_volumes( client, bucket, station, date, None).await?;
        volumes.extend( vs.into_iter().filter( |v| v.scan_time >= since && v.scan_time <= until));
        date = date.succ_opt().ok_or( op_failed!("invalid date"))?;
    }
    Ok(volumes)
}

/// the stations that have volumes for a given date
pub async fn list_stations (client: &S3Client, bucket: &str, date: NaiveDate)->Result<Vec<String>> {
    let prefix = format!("{}/", date.format("%Y/%m/%d"));
    let prefixes = s3::list_s3_prefixes( client, bucket, &prefix, "/").await?;
    Ok( prefixes.iter().filter_map( |p| p.strip_prefix( &prefix)).map( |p| p.trim_end_matches('/').to_string()).collect())
}

/// download a volume into memory
pub async fn download_volume (client: &S3Client, bucket: &str, volume: &VolumeInfo)->Result<Vec<u8>> {
    Ok( s3::get_s3_object_bytes( client, bucket, &volume.key).await? )
}

/// decode a (downloaded) Level II volume
pub fn decode_volume (data: Vec<u8>)->Result<nexrad::model::data::Scan> {
    Ok( nexrad::load( data)? )
}

/* #endregion volumes */

/* #region timing *****************************************************************************************************/

/// scan interval and publication delay statistics of a station, computed from the volumes of a S3 listing
/// Note this is not just station but also weather specific (i.e. variable). In order to minimize latency we need to compute it
#[derive(Debug,Clone,PartialEq)]
pub struct VolumeTiming {
    pub count: usize,
    pub first_scan: DateTime<Utc>,
    pub last_scan: DateTime<Utc>,
    pub last_interval: Option<Duration>,     // between the last two scans (most relevant for the next scan)
    pub interval_min: Option<Duration>,
    pub interval_median: Option<Duration>,
    pub interval_max: Option<Duration>,
    pub delay_min: Option<Duration>,         // publication delay (last-modified - scan start)
    pub delay_median: Option<Duration>,
    pub delay_max: Option<Duration>,
}

impl VolumeTiming {
    /// compute timing statistics from volumes (which don't need to be sorted). None if there are no volumes
    pub fn from_volumes (volumes: &[VolumeInfo])->Option<Self> {
        if volumes.is_empty() { return None }
        let mut times: Vec<DateTime<Utc>> = volumes.iter().map( |v| v.scan_time).collect();
        times.sort();
        times.dedup();

        let mut intervals: Vec<Duration> = times.windows(2).filter_map( |w| (w[1] - w[0]).to_std().ok()).collect();
        let last_interval = intervals.last().cloned();
        intervals.sort();

        let mut delays: Vec<Duration> = volumes.iter().filter_map( |v| v.publication_delay()).collect();
        delays.sort();

        Some( VolumeTiming {
            count: times.len(),
            first_scan: times[0],
            last_scan: *times.last().unwrap(),
            last_interval,
            interval_min: intervals.first().cloned(),
            interval_median: median( &intervals),
            interval_max: intervals.last().cloned(),
            delay_min: delays.first().cloned(),
            delay_median: median( &delays),
            delay_max: delays.last().cloned(),
        })
    }

    /// expected availability (publication) time of the next volume: last scan + last interval + median publication
    /// delay. Missing values are replaced by the given defaults
    pub fn next_expected (&self, default_interval: Duration, default_delay: Duration)->DateTime<Utc> {
        self.last_scan + self.last_interval.unwrap_or( default_interval) + self.delay_median.unwrap_or( default_delay)
    }
}

fn median (sorted: &[Duration])->Option<Duration> {
    if sorted.is_empty() { None } else { Some( sorted[ sorted.len() / 2]) }
}

/* #endregion timing */

/* #region products ***************************************************************************************************/

/// a computed product time step of a station
#[derive(Debug,Clone,PartialEq)]
pub struct NexradEntry {
    pub station: String,
    pub product: NexradProduct,
    pub scan_time: DateTime<Utc>,
    pub path: PathBuf, // product GeoTIFF in our cache
}

impl NexradEntry {
    pub fn new (cache_dir: &Path, station: &str, product: NexradProduct, scan_time: DateTime<Utc>)->Self {
        let path = entry_path( cache_dir, station, product, &scan_time);
        NexradEntry { station: station.to_string(), product, scan_time, path }
    }

    /// the (unique) WMS layer name of this entry
    pub fn layer_name (&self)->String { layer_name( &self.station, self.product, &self.scan_time) }

    pub fn json_value (&self)->serde_json::Value {
        serde_json::json!({ "date": self.scan_time.timestamp_millis(), "layer": self.layer_name() })
    }
}

pub fn date_tag (date: &DateTime<Utc>)->String {
    date.format("%Y%m%dT%H%M%SZ").to_string()
}

pub fn parse_date_tag (tag: &str)->Option<DateTime<Utc>> {
    NaiveDateTime::parse_from_str( tag, "%Y%m%dT%H%M%SZ").ok().map( |dt| dt.and_utc())
}

/// WMS layer name of a product time step, e.g. `KMUX_bref_20261117T001157Z`
pub fn layer_name (station: &str, product: NexradProduct, scan_time: &DateTime<Utc>)->String {
    format!("{station}_{}_{}", product.name(), date_tag( scan_time))
}

/// cache path of a product GeoTIFF: `<cache_dir>/<station>/<product>/<layer_name>.tif`
pub fn entry_path (cache_dir: &Path, station: &str, product: NexradProduct, scan_time: &DateTime<Utc>)->PathBuf {
    cache_dir.join( station).join( product.name()).join( format!("{}.tif", layer_name( station, product, scan_time)))
}

/// cache path of a (kept) Level II volume: `<cache_dir>/<station>/volumes/<volume name>`
pub fn volume_path (cache_dir: &Path, volume: &VolumeInfo)->PathBuf {
    cache_dir.join( &volume.station).join( "volumes").join( volume.name())
}

/* #endregion products */
