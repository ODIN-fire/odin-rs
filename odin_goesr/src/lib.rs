/*
 * Copyright (c) 2024, United States Government, as represented by the
 * Administrator of the National Aeronautics and Space Administration.
 * All rights reserved.
 *
 * The ODIN - Open Data Integration Framework is licensed under the
 * Apache License, Version 2.0 (the "License"); you may not use this file
 * except in compliance with the License. You may obtain a copy of the
 * License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */
#![feature(trait_alias,slice_take,duration_constructors)]
#![allow(unused)]

#[doc = include_str!("../doc/odin_goesr.md")]

use std::{f32::NAN, fmt::{Debug,Display}, fs::File, io::Write, ops::Deref, path::{Path,PathBuf}, sync::Arc, time::Duration};
use std::collections::VecDeque;
use serde::{Deserialize,Serialize};
use odin_common::{datetime::Dated, geo::LatLon};
use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, Timelike, Utc};
use uom::si::{area::square_meter, f32::Time, length::meter, power::milliwatt, thermodynamic_temperature::kelvin};
use uom::si::f32::{Power,ThermodynamicTemperature, Area, Length};
use futures::Future;
use regex::Regex;
use lazy_static::lazy_static;

use odin_actor::ActorHandle;
use odin_actor::prelude::*;
use odin_actor::error;
use odin_common::{if_let};
use odin_common::{*,fs::remove_old_files,datetime::full_hour,ranges::LinearRange};
use odin_common::s3::{S3Client,S3Object,create_s3_client,get_s3_objects,download_s3_object};
use odin_gdal::{Dataset, Metadata, MetadataEntry, GdalValueType}; // gdal re-exports
use odin_gdal::gdal::{DatasetOptions,GdalOpenFlags};
use odin_gdal::{GridPoint, find_grid_points_in_slice, get_grid_point_values, get_linear_range, nc_dataset, quiet_nc_dataset};

mod errors;
pub use errors::*;

pub mod actor;
pub use actor::*;

pub mod live_importer;
pub use live_importer::*;

pub mod assets;

mod geo;
use geo::{GoesRBoundingBox,GoesRProjection,get_bounds};

/* #region Goes R data structures  ***************************************************************************/

#[derive(Debug,PartialEq,Clone)]
pub struct GoesRData {
    pub sat_id: u8,
    pub file: PathBuf,
    pub source: Arc<String>,
    pub date: DateTime<Utc>
}

#[derive(Debug,Clone, Serialize)]
pub struct GoesRHotSpot {
    pub sat_id: u8,
    pub date: DateTime<Utc>,
    pub position: LatLon,
    pub bounds: GoesRBoundingBox,
    pub bright: ThermodynamicTemperature, 
    pub frp: Power, 
    pub area: Area,
    pub dqf: u8,
    pub mask: u16,
    pub source: Arc<String>, // don't duplicate
    pub pixel_size: Length
}

impl GoesRHotSpot {
    pub fn new (data: &GoesRData, mask: u16, bright:u16, frp:f32, dqf:u8, area:u16, bounds: GoesRBoundingBox, center:LatLon)->Self {
        GoesRHotSpot {
            sat_id: data.sat_id,
            date: data.date,
            //location info
            position: center, 
            bounds: bounds,
            // data info
            bright: ThermodynamicTemperature::new::<kelvin>(bright.into()), 
            frp: Power::new::<milliwatt>(frp.into()), 
            area: Area::new::<square_meter>(area.into()),
            dqf: dqf,
            mask: mask,
            // product info
            source: data.source.clone(),
            pixel_size: Length::new::<meter>(2000.0)
          }
    }
}

#[derive(Debug,Clone, Serialize)] // to do: add to json, to json pretty
pub struct GoesRHotSpots {
    pub sat_id: u8,
    pub date: DateTime<Utc>,
    pub source: Arc<String>,
    pub hotspots: Vec<GoesRHotSpot>
}

impl GoesRHotSpots {
    pub fn new(data: &GoesRData, hotspot_vec: Vec<GoesRHotSpot>) -> Self {
        GoesRHotSpots {
            date: data.date.clone(),
            sat_id: data.sat_id.clone(),
            source: data.source.clone(),
            hotspots: hotspot_vec
        }
    }
    pub fn to_json_pretty (&self)->Result<String> {
        Ok(serde_json::to_string_pretty( &self )?)
    }
    pub fn to_json (&self)->Result<String> {
        Ok(serde_json::to_string( &self )?)
    }
}

#[derive(Debug,Clone, Serialize)]
pub struct HotspotStore {
    hotspots: VecDeque<GoesRHotSpots>,
    max_capacity: usize
}
impl HotspotStore {
    pub fn new(capacity: usize) -> Self {
        HotspotStore {
            hotspots:VecDeque::with_capacity(capacity),
            max_capacity:capacity
        }
    }
    pub fn update_hotspots(&mut self, new_hotspots: GoesRHotSpots) -> () {
        // if vec is not max add in - assume update is from newer date
        if self.hotspots.len() < self.max_capacity {
            self.hotspots.push_front(new_hotspots);
        } else {
            // remove last, add newest
            self.hotspots.pop_back();
            self.hotspots.push_front(new_hotspots);
        }
    }

    pub fn initialize_hotspots(&mut self, init_hotspots: Vec<GoesRHotSpots>) -> () {
        for hs in init_hotspots {
            self.hotspots.push_front(hs);
        }
    }
    pub fn to_json_pretty (&self)->Result<String> {
        Ok(serde_json::to_string_pretty( &self.hotspots )?)
    } 
}

/* #endregion GoesR data structure */

/* #region GOES-R filename encoding *************************************************************************************/

lazy_static! {
    static ref FILENAME_RE: Regex = Regex::new(r#"(?:.*/)?(.*)_([^-]*)-([^-]*)-([^-]+)-(.*)_G(.*)_s(.*)_e(.*)_c(.*)\.(.*)"#).unwrap();
    static ref DTG_RE: Regex = Regex::new(r#"(\d\d\d\d)(\d\d\d)(\d\d)(\d\d)(\d\d)(\d)"#).unwrap();
}

/// file info as encoded in files downloaded from AWS S3
/// see https://www.goes-r.gov/products/docs/PUG-L2+-vol5.pdf (pg 608)
/// schema:
///         «sys_env» _ «instrument» - «level» - «product» - «mode» _G «sat_id» _s «start-time» _e «end-time» _c «create-time» .nc
/// 
/// times are in UTC and specified as
///        yyyy : year
///         ddd : day of year (001-366)
///          HH : UTC hour of day (00-23)
///          MM : minutes (00-59)
///          SS : seconds (00-59)
///           s : tenths of second (0-9)
/// 
/// example: `OR_ABI-L2-FDCC-M6_G16_s20241380556172_e20241380558545_c20241380559122.nc`
#[derive(Debug)]
pub struct GoesrFileInfo {
    pub sys_env: String, // e.g. "OR": operational realtime
    pub instrument: String, // e.g. "ABI"
    pub level: String, // e.g. "L2"
    pub product: String, // e.g. FDCC
    pub mode: String, // e.g. "M6"
    pub sat_id: u8, // e.g. 16
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub create_time: DateTime<Utc>,
}

/// parse GoesrFileInfo from given pathname
pub fn parse_filename (path: impl AsRef<Path>)->Option<GoesrFileInfo> {
    let path: &Path = path.as_ref();
    let filename = path.file_name()?.to_str()?;

    if_let! {
        Some(cap) = FILENAME_RE.captures(filename),
        11 = cap.len(),
        sys_env = cap[1].to_string(),
        instrument = cap[2].to_string(),
        level = cap[3].to_string(),
        product = cap[4].to_string(),
        mode = cap[5].to_string(),
        Ok(sat_id) = cap[6].parse::<u8>(),
        Some(start_time) = parse_goesr_dtg( &cap[7]),
        Some(end_time) = parse_goesr_dtg( &cap[8]),
        Some(create_time) = parse_goesr_dtg(&cap[9]) => {
            return Some( GoesrFileInfo{sys_env,instrument,level,product,mode,sat_id,start_time,end_time,create_time} )
        }
    }
    None
}

pub fn parse_goesr_dtg (s: &str)->Option<DateTime<Utc>> {
    if_let! {
        Some(cap) = DTG_RE.captures(s),
        7 = cap.len(),
        Ok(year) = cap[1].parse::<i32>(),
        Ok(doy) = cap[2].parse::<u32>(),
        Ok(hour) = cap[3].parse::<u32>(),
        Ok(min) = cap[4].parse::<u32>(),
        Ok(sec) = cap[5].parse::<u32>(),
        Ok(dec) = cap[6].parse::<u32>(),
        Some(nd) = NaiveDate::from_yo_opt( year, doy),
        Some(nt) = NaiveTime::from_hms_milli_opt(hour, min, sec, dec * 100) => {
            return Some( NaiveDateTime::new( nd, nt).and_utc() )
        }
    }
    None
}

pub fn parse_goesr_create_dtg (path: impl AsRef<Path>)->Option<DateTime<Utc>> {
    let path: &Path = path.as_ref();
    let filename = path.file_name()?.to_str()?;
    filename.rfind("_c").and_then(|idx| parse_goesr_dtg(&filename[idx+2..]))
}

/* #endregion GOES-R filename encoding */

/* #region S3 support *************************************************************************************************/

/// the S3 object prefix (some sort of a path) for GoesR. Built from year, day-of-year and hour
fn get_prefix (dt: DateTime<Utc>, source: &str)->String {
    format!("{}/{}/{:03}/{:02}/", source, dt.year(), dt.ordinal(), dt.hour())
}

/// return all objects within the given duration, in ascending time order (newest last)
/// Use this for getting initial data
pub async fn get_most_recent_objects (client: &S3Client, bucket: &str, source: &str, dur: Duration, now: DateTime<Utc>) -> Result<Vec<S3Object>> {
    let dt_start = now - dur;
    let hours = dur.as_secs() as i64/ 3600;
    let mut objects: Vec<S3Object> = Vec::with_capacity( 12 * (hours+1) as usize); // assuming update interval is 5min

    for h in (0..=hours).rev() {
        let dt = now - TimeDelta::hours(h);
        let prefix = get_prefix( dt, source);
        let mut objs = get_s3_objects( client, bucket, &prefix, None).await?;

        for o in objs {
            if o.is_newer(dt_start)  {
                objects.push(o)
            }
        }
    }

    Ok(objects)
}

/// return all objects since the given last one, in ascending time order (newest last)
/// Use this for getting updates
pub async fn get_objects_since_last (client: &S3Client, bucket: &str, source: &str, last_obj: &S3Object, now: DateTime<Utc>)  -> Result<Vec<S3Object>> {
    let key = last_obj.key().ok_or(OdinGoesRError::NoObjectKeyError())?;
    let dt_start = parse_goesr_create_dtg(key).ok_or(OdinGoesRError::NoObjectDateError())?;
    let hours = (full_hour(now) - full_hour(dt_start)).num_hours();
    let mut objects: Vec<S3Object> = Vec::with_capacity( 12 * (hours+1) as usize); // assuming update interval is 5min

    for h in (0..=hours).rev() {
        let dt = now - TimeDelta::hours(h);
        let prefix = get_prefix( dt, source);
        let marker = if h == hours { Some(key) } else { None };

        let mut objs = get_s3_objects( client, bucket, &prefix, marker).await?;
        for o in objs {
            if o.is_newer(dt_start) && o.is_older_or_equal(now) {
                objects.push(o)
            }
        }
    }

    Ok(objects)
}

// get all S3Objects either from last downloaded one or as a fallback since the provided DateTime<Utc>
pub async fn get_objects_since (client: &S3Client, bucket: &str, source: &str, last_obj: &Option<S3Object>, dt: DateTime<Utc>, now: DateTime<Utc>)->Result<Vec<S3Object>> {
    if let Some(last_obj) = last_obj {
        get_objects_since_last( &client, bucket, &source, &last_obj, now).await
    } else {
        get_most_recent_objects( &client, bucket, &source, (now - dt).to_std()?, now).await
    }
}

pub async fn download_and_read_objects (client: &S3Client, bucket: &str, source: &Arc<String>, sat_id: u8, data_dir: &PathBuf, objs: &Vec<S3Object>) -> Result<Vec<GoesRHotSpots>> {
    let mut hotspots: Vec<GoesRHotSpots> = Vec::with_capacity(objs.len());

    for obj in objs {
        let gdata = get_goesr_data( client, obj, data_dir, bucket, source.clone(), sat_id).await?;
        match read_goesr_data( &gdata) {
            Ok(hs) => hotspots.push(hs),
            Err(e) => warn!("error parsing GOES-R data: {e:?}")
        }
    }

    Ok( hotspots )
}

pub async fn get_goesr_data (client: &S3Client, obj: &S3Object, path: &PathBuf, bucket: &str, source:Arc<String>, sat_id:u8) -> Result<GoesRData>{
    if obj.is_dated() {
        let date = obj.date();
        let file = download_s3_object(client, bucket, obj, path).await?;
        let data = GoesRData{sat_id, file, source, date};
        Ok(data)
    } else {
        Err( OdinGoesRError::NoObjectDateError())
    }
}


/* #endregion S3 support */

/* #region hotspot parsing *************************************************************************************************/

fn find_fire_pixels_in_slice (i1: usize, row: &[u16], grid_points: &mut Vec<GridPoint<u16>>) {
    for i0 in 0..row.len() {
        let mask = row[i0];
        if mask >= 10 && mask <= 35 {
            grid_points.push( GridPoint{i0,i1,value: mask})
        }
    }
}

pub fn read_goesr_data (data: &GoesRData) -> Result<GoesRHotSpots> {
    let mask_ds = quiet_nc_dataset( &data.file,"Mask")?;
    let proj = GoesRProjection::from_dataset( &mask_ds)?;
    let hs = find_grid_points_in_slice( &mask_ds, 1, find_fire_pixels_in_slice)?;

    let area: Vec<f32> = get_grid_point_values( &quiet_nc_dataset( &data.file, "Area")?, 1, Some(NAN), &hs)?;
    let power: Vec<f32> = get_grid_point_values( &quiet_nc_dataset( &data.file, "Power")?, 1, Some(NAN), &hs)?;
    let temp: Vec<f32> = get_grid_point_values( &quiet_nc_dataset( &data.file, "Temp")?, 1, Some(NAN), &hs)?;
    let dqf: Vec<u8> = get_grid_point_values( &quiet_nc_dataset( &data.file, "DQF")?, 1, None, &hs)?;

    let x_range = get_linear_range::<f64>( &nc_dataset(&data.file,"x")?, 1)?;
    let y_range = get_linear_range::<f64>( &nc_dataset(&data.file,"y")?, 1)?;

    let mut hotspots: Vec<GoesRHotSpot> = Vec::with_capacity(hs.len());
    for (i,p) in hs.iter().enumerate() {
        let center = proj.lat_lon_from_instrument_angles(x_range.at(p.i0), y_range.at(p.i1));
        let bounds = get_bounds( &proj, &x_range, &y_range, &p);

        if !temp[i].is_nan() {
            let hotspot = GoesRHotSpot::new( data, p.value, temp[i] as u16, power[i], dqf[i], area[i] as u16, bounds, center);
            hotspots.push( hotspot)
        }
    }

    Ok( GoesRHotSpots::new( data, hotspots) )
}

/* #endregion hotspot parsing */
