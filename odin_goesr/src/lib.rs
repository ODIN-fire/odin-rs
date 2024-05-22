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
#![feature(trait_alias,slice_take)]
#![allow(unused)]

use std::{fmt::{Debug,Display},f32::NAN,sync::Arc,fs::File, path::{Path,PathBuf}, time::Duration, io::Write};
use std::collections::VecDeque;
use aws_smithy_types_convert::date_time::DateTimeExt;
use serde::{Deserialize,Serialize};
use odin_common::geo::LatLon;
use chrono::{NaiveTime,NaiveDate,NaiveDateTime,Datelike, DateTime, Utc, Timelike};
use uom::si::{area::square_meter,length::meter,power::milliwatt,thermodynamic_temperature::kelvin};
use uom::si::f32::{Power,ThermodynamicTemperature, Area, Length};
use aws_sdk_s3::{types::Object, Client, operation::list_objects::builders::ListObjectsFluentBuilder};
use aws_config::{Region,meta::region::RegionProviderChain};
use futures::Future;
use regex::Regex;
use lazy_static::lazy_static;

use odin_actor::ActorHandle;
use odin_actor::prelude::*;
use odin_actor::error;
use odin_common::if_let;
use odin_common::fs::remove_old_files;
use odin_common::{*,datetime::parse_utc_datetime_from_yyyydddhhmmss,ranges::LinearRange};
use odin_gdal::{Dataset, Metadata, MetadataEntry, GdalValueType}; // gdal re-exports
use odin_gdal::gdal::{DatasetOptions,GdalOpenFlags};
use odin_gdal::{GridPoint, find_grid_points_in_slice, get_grid_point_values, get_linear_range, nc_dataset, quiet_nc_dataset};

mod errors;
pub use errors::*;

pub mod actor;
pub use actor::*;

pub mod live_importer;
pub use live_importer::*;

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
    static ref FILENAME_RE: Regex = Regex::new(r#"(.*)_([^-]*)-([^-]*)-([^-]+)-(.*)_G(.*)_s(.*)_e(.*)_c(.*)\.(.*)"#).unwrap();
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

/* #endregion GOES-R filename encoding */

/* #region S3 support *************************************************************************************************/

pub async fn create_s3_client(region: String) -> Result<Client> {
    let region_provider = RegionProviderChain::first_try( Region::new( region));
    let aws_config = aws_config::from_env().no_credentials().region(region_provider).load().await; // add anonymous creditials
    Ok( Client::new(&aws_config) ) 
}

pub async fn get_inital_objects (client: &Client, dt: DateTime<Utc>, bucket: &str, source: &str, num_obj: usize) -> Result<Vec<Object>> {
    let prefix = get_prefix( dt, source); 
    let mut objects = get_multiple_objects( client, bucket, &prefix, num_obj).await?;

    if objects.len() < num_obj { // we didn't get enough objs for this hour - fill up from previous hour
        let num_obj = num_obj - objects.len();
        let prefix = get_prefix( dt - Duration::from_secs(3600), source);
        let mut more_objects = get_multiple_objects( client, bucket, &prefix, num_obj).await?;
        objects.append( &mut more_objects);
    }

    Ok(objects)
}

fn get_prefix (dt: DateTime<Utc>, source: &str)->String {
    format!("{}/{}/{:03}/{:02}/", source, dt.year(), dt.ordinal(), dt.hour())
}

pub async fn get_multiple_objects (client: &Client, bucket: &str, prefix: &str, num_obj: usize) -> Result<Vec<Object>> {
    let builder = client.list_objects().bucket(bucket).prefix(prefix);
    let result = builder.send().await?;
    let objs = result.contents();

    let n = objs.len();
    let last_objs = if n > num_obj { &objs[n - num_obj..n] } else { objs };
    Ok( last_objs.to_vec() )
}

pub async fn get_most_recent_object (client: &Client, dt: DateTime<Utc>, bucket: &str, source: &str, prev_key: Option<&String>) -> Result<Option<Object>> {
    fn get_builder (client: &Client, bucket: &str, prefix: String, prev_key: Option<&String>)->ListObjectsFluentBuilder {
        let mut builder = client.list_objects().bucket(bucket).prefix(prefix);
        if let Some(key) = prev_key { builder.marker(key) } else { builder }
    }

    let mut result = get_builder( client, bucket, get_prefix( dt, source), prev_key).send().await?;
    if result.contents.is_none() { // try previous hour but don't get recursive (this is an async fn)
        result = get_builder( client, bucket, get_prefix( dt - Duration::from_secs(3600), source), prev_key).send().await?;
    }
    
    if result.contents.is_none() { 
        Err( OdinGoesRError::NoObjectError("no object found in last hour".into()))
    } else {
        Ok( result.contents().last().map(|o| o.clone()) )
    }
}

pub async fn get_goesr_data (client: &Client, obj: Object, path: &PathBuf, bucket: &str, source:Arc<String>, sat_id:u8) -> Result<GoesRData>{
    if let Some(date) = obj.last_modified {
        let date = date.to_chrono_utc()?;
        let file = download_object(client, bucket, obj, path).await?;
        let data = GoesRData{sat_id, file, source, date};
        Ok(data)
    } else {
        Err( OdinGoesRError::NoObjectDateError())
    }
}

async fn download_object (client: &Client, bucket: &str, object: Object, path: &PathBuf) -> Result<PathBuf>{
    if let Some(key) = object.key {
        let file_name = key.split("/").collect::<Vec<&str>>().last().copied().unwrap();
        let file_path = path.join(file_name);
        let mut file = File::create(&file_path)?;

        let mut object = client
            .get_object()
            .bucket(bucket)
            .key(&key)
            .send()
            .await?; 

        while let Some(bytes) = object.body.try_next().await? {
            file.write_all(&bytes)?;
        }
        Ok(file_path)
    } else {
        Err(OdinGoesRError::NoObjectKeyError())
    }
}

/* #endregion S3 support */

/* #region hotspot parsing *************************************************************************************************/


/// read hotspot data from GOES-R fire product as documented in
/// `GoesR ABI L2 Fire (Hot Spot Characterization) data product` ("ABI-L2-FDCC")
/// see https://www.goes-r.gov/products/docs/PUG-L2+-vol5.pdf (pg 472) for details
/*
pub fn _read_goesr_data (data: &GoesRData) -> Result<GoesRHotSpots> {
    let lat_lon_grid = get_lat_lon_grid(&data.file)?;

    let fire_pixels = find_2d_grid_points( netcdf_path( data, "Mask").as_str(), 1, is_valid_fire_pixel)?;

    let area_vals: Vec<u16>  = get_2d_grid_point_values( netcdf_path( data, "Area").as_str(), 1, &fire_pixels)?;
    let temp_vals: Vec<u16>  = get_2d_grid_point_values( netcdf_path( data, "Temp").as_str(), 1, &fire_pixels)?;
    let power_vals: Vec<f32> = get_2d_grid_point_values( netcdf_path( data, "Power").as_str(), 1, &fire_pixels)?;
    let dqf_vals: Vec<u8>    = get_2d_grid_point_values( netcdf_path( data, "DQF").as_str(), 1, &fire_pixels)?;

    let hotspots = fire_pixels.iter().enumerate().fold( Vec::<GoesRHotSpot>::with_capacity(fire_pixels.len()), |mut acc, (i,p)| {
        let center = get_lat_lon( &lat_lon_grid, p.i0, p.i1);
        let bounds = get_bounds( &lat_lon_grid, p.i0, p.i1);
        let hotspot = GoesRHotSpot::new( data, p.value, temp_vals[i], power_vals[i], dqf_vals[i], area_vals[i], bounds, center);
        acc.push( hotspot);
        acc
    });

    Ok( GoesRHotSpots::new( data, hotspots) )
}
*/

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
