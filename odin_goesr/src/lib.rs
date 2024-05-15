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
#![feature(trait_alias)]
#![allow(unused)]

use core::result::Result::Ok;
use futures::future::join_all;
use odin_actor::ActorHandle;
use odin_actor::prelude::*;
use odin_actor::error;
use odin_common::fs::remove_old_files;

use std::sync::Arc;
use std::collections::VecDeque;
use std::{fs::File, path::PathBuf, time::Duration, io::Write};
use std::collections::HashMap;
use aws_smithy_types_convert::date_time::DateTimeExt;
use serde::{Deserialize,Serialize};
use odin_common::geo::LatLon;
use chrono::{Datelike, DateTime, Utc, Timelike};
use uom::si::area::square_meter;
use uom::si::f32::{Power,ThermodynamicTemperature, Area, Length};
use uom::si::length::meter;
use uom::si::power::milliwatt;
use uom::si::thermodynamic_temperature::kelvin;
use aws_sdk_s3::{types::Object, Client};
use aws_config::meta::region::RegionProviderChain;
use aws_config::Region;
use gdal::Dataset;
use paste::paste;
use futures::Future;

mod errors;
pub mod actor;
pub use errors::*;
pub mod geo;
pub mod live_importer;
use geo::*;
use actor::*;

/* #region Goes R data structures  ***************************************************************************/

#[derive(Debug,PartialEq,Clone)]
pub struct GoesRData {
    pub sat_id: u8,
    pub file: PathBuf,
    pub product: GoesRProduct,
    pub date: DateTime<Utc>
}
#[derive(Serialize,Deserialize,Debug,PartialEq,Clone)]
pub struct GoesRProduct {
    pub name: String,
    pub bucket: String,
    pub history: String
}

#[derive(Debug,Clone, Serialize)]
pub struct GoesRHotSpot {
    pub sat_id: u8,
    pub date: DateTime<Utc>,
    pub position: LatLon,
    //center: LatLog, 
    pub bounds: GoesRBoundingBox,
    pub bright: ThermodynamicTemperature, 
    pub frp: Power, 
    pub area: Area,
    pub dqf: u8,
    pub mask: u16,
    pub source: String,
    // fixed 
    pub sensor: String,
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
            source: data.product.name.clone(),
            sensor: String::from("ABI"),
            pixel_size: Length::new::<meter>(2000.0)
          }
    }
}

#[derive(Debug,Clone, Serialize)] // to do: add to json, to json pretty
pub struct GoesRHotSpots {
    pub sat_id: u8,
    pub date: DateTime<Utc>,
    pub source: String,
    pub hotspots: Vec<GoesRHotSpot>
}

impl GoesRHotSpots {
    pub fn new(data: &GoesRData, hotspot_vec: Vec<GoesRHotSpot>) -> Self {
        GoesRHotSpots {
            date: data.date.clone(),
            sat_id: data.sat_id.clone(),
            source: data.product.name.clone(),
            hotspots: hotspot_vec
        }
    }
    pub fn to_json_pretty (&self)->Result<String> {
        Ok(serde_json::to_string_pretty( &self )?)
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


#[derive(Serialize,Deserialize,Debug,PartialEq,Clone)]
pub struct GoesRImportActorConfig {
    pub polling_interval: Duration,
    pub satellite: u8,
    pub keep_files: bool,
    pub s3_region: String,
    pub product: GoesRProduct,
    pub init_records: usize,
    pub max_records:usize,
    pub max_age: Duration
}
/* #endregion GoesR data structure */

/* #region s3 getters *************************************************************************************************/
pub fn get_bucket(sat_id:&u8) -> String{
    format!("noaa-goes{}", sat_id)
}

async fn get_multiple_objects(client: &Client, bucket: &String, prefix: &String, num_obj: &usize) -> Result<Option<Vec<Object>>> {
    let builder = client.list_objects().bucket(bucket).prefix(prefix);
    let resp = builder.send().await?; 
    let objects = resp.contents;
    let mut init_objs: Vec<Object> = vec![];
    if let Some(object_vec) = objects {
        if object_vec.len() <= *num_obj { // add all of them to the vector
            init_objs.extend(object_vec);
        } else { // take last num_obj objects from the vector - we can assume it is sorted in order since it is UTF-8 sorted https://docs.aws.amazon.com/AmazonS3/latest/userguide/ListingKeysUsingAPIs.html
            init_objs.extend(object_vec[object_vec.len()-num_obj .. object_vec.len()].to_vec());
        }
        Ok(Some(init_objs))
    } else {
        Ok(None)
    }
}

pub fn get_prefix_last_hour (dt: &DateTime<Utc>, product:&GoesRProduct) -> String {
    let mut prev_hour = (dt.hour() as i16 -1 as i16) as i16;
    let mut day = dt.ordinal();
    if prev_hour<0 {
        prev_hour = 23;
        day = day-1;
    }
    format!("{}/{}/{:03}/{:02}/", product.name, dt.year(), day, prev_hour)
    
}

pub async fn get_multiple_last_hour_objects (client: &Client, dt: &DateTime<Utc>, bucket:&String, product:&GoesRProduct,  num_obj: &usize) -> Result<Option<Vec<Object>>> {
    let prefix_last_hour  = get_prefix_last_hour (&dt, &product);
    let last_hour_objects = get_multiple_objects(&client, bucket, &prefix_last_hour, &num_obj).await?;
    Ok(last_hour_objects)
}

pub async fn get_inital_objects(client:&Client, dt: chrono::DateTime<Utc>, product:&GoesRProduct, sat_id:&u8, num_obj: usize) -> Result<Vec<Object>> {
    let prefix = format!("{}/{}/{:03}/{:02}/", product.name, dt.year(), dt.ordinal(), dt.hour()); // https://stackoverflow.com/questions/76651472/do-rust-s3-sdk-datetimes-work-with-chrono
    let bucket = format!("noaa-goes{}", sat_id);
    let objects = get_multiple_objects(&client, &bucket, &prefix, &num_obj).await?;
    let mut num_prev_hour_objs = num_obj.clone();
    let mut init_objs = vec![];
    if let Some(objs) = objects {
        num_prev_hour_objs = num_prev_hour_objs - objs.len();
        for obj in objs.into_iter() {
            init_objs.push(obj);
        }
    } 
    if num_prev_hour_objs > 0 {
        // try previous hour for case when we start the program before the data is up for the current hour (e.g., start at 5:00p - get error of no objects)
        let last_hour_objects = get_multiple_last_hour_objects(&client, &dt, &bucket, product, &num_prev_hour_objs).await?;
        if let Some(objs) = last_hour_objects {
            for obj in objs.into_iter() {
                init_objs.push(obj);
            }
        } 
    }
    Ok(init_objs)
}

async fn get_most_recent_object(client: &Client, bucket: &String, prefix: &String, prev_obj: Option<&Object>) -> Result<Option<Object>> {
    let mut builder = client.list_objects().bucket(bucket).prefix(prefix);
    if let Some(prev_obj_exists) = prev_obj {
        if let Some(key) = prev_obj_exists.key() {
            builder = builder.marker(key);
        }
    };
    let resp = builder.send().await?; 
    let objects = resp.contents;
    if let Some(object_vec) = objects {
        let last_obj = get_most_recent_obj_from_vec(&object_vec)?.clone();
        Ok(Some(last_obj))
    } else {
        Ok(None)
    }
}

pub async fn get_goesr_data(client: &Client, obj: Object, destination: &PathBuf, product:&GoesRProduct, sat_id:u8) -> Result<GoesRData>{
    let obj_dt = obj.last_modified.clone().unwrap();
    let file = download_object(&client, &get_bucket(&sat_id), obj, destination).await?;
    let data = GoesRData{sat_id: sat_id, file:file, product:product.clone(), date: obj_dt.to_chrono_utc().unwrap()};
    Ok(data)
}

async fn download_object(client: &Client, bucket: &String, object: Object, destination: &PathBuf) -> Result<PathBuf>{
    if let Some(key) = object.key {
        let file_name = key.split("/").collect::<Vec<&str>>().last().copied().unwrap();
        let file_path = PathBuf::from(destination).join(file_name);
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

pub async fn get_last_hour_objects (client: &Client, dt: &DateTime<Utc>, bucket:&String, product:&GoesRProduct, prev_obj: Option<&Object>) -> Result<Option<Object>> {
    let prefix_last_hour  = get_prefix_last_hour (&dt, &product);
    let last_hour_object = get_most_recent_object(&client, bucket, &prefix_last_hour, prev_obj).await?;
    Ok(last_hour_object)
}

pub fn get_most_recent_obj_from_vec(objs: &Vec<Object>) -> Result<&Object> {
    let mut date = None;
    let mut last_obj = None;
    for obj in objs.iter() {
        if let Some(dt) = date {
            if let Some(obj_date) = obj.last_modified {
                if obj_date < dt {
                    continue;
                }
            }
        }
        date = obj.last_modified.clone();
        last_obj = Some(obj);
    }
    Ok(last_obj.unwrap()) // we know there is atleast one object so we can unwrap
}
/* #endregion s3 getters */

/* #region hotspot processing *************************************************************************************************/
macro_rules! define_get_variable {
    ($f:expr, $r:ty, $n:expr) => { paste!{
        fn [<get_ $f _vals>]  (data: &GoesRData, x_vals: &Vec<usize>, y_vals: &Vec<usize>) -> Result<Vec<$r>> {
            let file = format!("NETCDF:{:?}:{:?}", data.file, $n);
            let ds = Dataset::open(file)?;
            let band = ds.rasterband(1)?;
            let mut vals: Vec<$r> = vec![]; 
            for i in 0..x_vals.len() {
                let band_slice = band.read_as_array::<$r>((0,0), (y_vals[i],x_vals[i]), (1,1), None)?;
                vals.push(band_slice[[0,0]]);
            }
            Ok(vals)
        }
    }
    };
}

define_get_variable! {"area", u16, "Area"}
define_get_variable! {"temp", u16, "Temp"}
define_get_variable! {"power", f32, "Power"}
define_get_variable! {"dqf", u8, "DQF"}

pub fn is_valid_fire_pixel (mask: u16) -> bool {
    return mask >= 10 && mask <= 35
} 

pub fn get_hotspots (data: &GoesRData, areas: Vec<u16>, masks: Vec<u16>, powers: Vec<f32>, dqfs: Vec<u8>, temps: Vec<u16>, centers: Vec<LatLon>, bounds: Vec<GoesRBoundingBox>) -> Result<Vec<GoesRHotSpot>> {
    let mut hotspots: Vec<GoesRHotSpot> = vec![];
    for i in 0..masks.len(){
        hotspots.push(GoesRHotSpot::new(&data, 
            masks[i], 
            temps[i],
             powers[i], 
             dqfs[i], 
             areas[i],
            bounds[i],
        centers[i]))
    }
    Ok(hotspots)
}

pub fn read_goesr_data(data: &GoesRData) -> Result<GoesRHotSpots> {
    let lat_lon_grid = get_lat_lon_grid(&data)?;
    let mut x_vals: Vec<usize> = vec![];
    let mut y_vals: Vec<usize> = vec![];
    let mut mask_vals:Vec<u16> = vec![];
    let mask_file = format!("NETCDF:{:?}:Mask", data.file);
    let ds = Dataset::open(mask_file)?;
    let mask_band = ds.rasterband(1)?;
    let mask = mask_band.read_as_array::<u16>((0,0), mask_band.size(), mask_band.size(), None)?;
    let mask_dim = mask.dim();
    for x in 0..mask_dim.0 {
        for y in 0..mask_dim.1 {
            let mask_val = mask[[x,y]];
            if is_valid_fire_pixel(mask_val){
                mask_vals.push(mask_val);
                x_vals.push(x);
                y_vals.push(y);
            }
        }
    }
    let area_vals = get_area_vals(&data, &x_vals, &y_vals)?;
    let temp_vals = get_temp_vals(&data, &x_vals, &y_vals)?;
    let power_vals = get_power_vals(&data, &x_vals, &y_vals)?;
    let dqf_vals = get_dqf_vals(&data, &x_vals, &y_vals)?;
    let centers = get_lat_lons(&x_vals, &y_vals, &lat_lon_grid)?;
    let bounds = get_bounds_vector(&x_vals, &y_vals, &lat_lon_grid)?;
    let hotspot_vec = get_hotspots(&data, area_vals, mask_vals, power_vals, dqf_vals, temp_vals, centers, bounds)?;
    let hotspots = GoesRHotSpots::new(&data, hotspot_vec);
    Ok(hotspots)
}
/* #endregion hotspot processing */
