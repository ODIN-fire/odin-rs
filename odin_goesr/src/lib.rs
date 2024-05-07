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

use futures::future::join_all;
use odin_actor::ActorHandle;
use odin_actor::prelude::*;
use odin_actor::error;

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
use std::time::Instant;
use futures::Future;

mod errors;
pub mod actor;
pub use errors::*;
pub mod geo;
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
            //center: None, 
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

#[derive(Serialize,Deserialize,Debug,PartialEq,Clone)]
pub struct GoesRImportActorConfig {
    pub polling_interval: Duration,
    pub satellite: u8,
    pub data_dir: PathBuf,
    pub keep_files: bool,
    pub s3_region: String,
    pub products: Vec<GoesRProduct>,
    pub init_records: usize,
    pub max_records:usize
}

pub trait GoesRDataImporter {
    fn start (&mut self, hself: ActorHandle<GoesRActorMsg>) -> impl Future<Output=Result<()>> + Send;
    // fn terminate (&mut self);
    // fn max_history(&self)->usize;
}

#[derive(Debug,Clone)]
pub struct LiveGoesRDataImporter {
    pub config: GoesRImportActorConfig,
    pub data_dir: PathBuf,
    pub task: Option<LiveGoesRDataAcquisitionTask>
}

impl LiveGoesRDataImporter {
    pub fn new (config: GoesRImportActorConfig) -> Self {
        LiveGoesRDataImporter {
            data_dir: config.data_dir.clone(),
            config: config,
            task: None
        }
    }
    async fn initialize  (&mut self, hself: ActorHandle<GoesRActorMsg>) -> Result<()> { 
        self.task = Some(LiveGoesRDataAcquisitionTask::new(self.config.clone(), hself).await);
        Ok(())
    }
}

impl GoesRDataImporter for LiveGoesRDataImporter {
    async fn start (&mut self, hself: ActorHandle<GoesRActorMsg>) -> Result<()> {
        self.initialize(hself).await?;
        if let Some(ref mut task) = self.task {
            task.spawn_data_acquitision_task().await; // this will send and initilize message, set up future downloads
        }
        Ok(())
    }

    // fn terminate (&mut self);
    // fn max_history(&self)->usize;
}


// init action
#[derive(Clone, Debug)]
pub struct LiveGoesRDataAcquisitionTask { 
    pub latest_objs: HashMap<String, Object>,
    pub sat_id: u8,
    pub polling_interval: Duration,
    pub s3_client:Client,
    pub products: Vec<GoesRProduct>,
    pub data_dir: PathBuf,
    pub init_records:usize,
    pub hself: ActorHandle<GoesRActorMsg>

}

impl LiveGoesRDataAcquisitionTask {

    pub async fn new(config:GoesRImportActorConfig, hself:ActorHandle<GoesRActorMsg>) -> Self {
        let region_provider = RegionProviderChain::first_try(Region::new(config.s3_region.clone()));
        let aws_config = aws_config::from_env().no_credentials().region(region_provider).load().await; // add anonymous creditials
        let s3_client = Client::new(&aws_config);
        let latest_objs:HashMap<String, Object> = HashMap::new();
        LiveGoesRDataAcquisitionTask {
            latest_objs: latest_objs,
            sat_id: config.satellite,
            polling_interval: config.polling_interval,
            s3_client: s3_client,
            products: config.products,
            data_dir: config.data_dir,
            init_records: config.init_records,
            hself: hself
        }
    }

    pub async fn initial_download(&mut self) -> Result<Vec<GoesRHotSpots>> {
        //downloads x amount of files
        // updates latest obj
        let product = &self.products[0];
        let dt = Utc::now();
        let num_obj=self.init_records;
        let init_objs = get_inital_objects(&self.s3_client, dt, product, &self.sat_id,  num_obj).await?;
        if init_objs.len() > 0 {
            let most_recent = get_most_recent_obj_from_vec(&init_objs)?;
            self.latest_objs.insert(product.name.clone(), most_recent.clone());
            let data = join_all(init_objs.iter().map(|x| async{get_goesr_data(&self.s3_client, x.clone(), &self.data_dir, product, self.sat_id.clone()).await})).await;
            let goesr_data: Result<Vec<GoesRData>> = data.into_iter().collect();
            let goesr_data_vec = goesr_data?;
            let hotspots_res:  Result<Vec<GoesRHotSpots>>  = goesr_data_vec.into_iter().map( |x| read_goesr_data(&x)).into_iter().collect();
            let hotspots = hotspots_res?;
            Ok(hotspots)
        } else {
            Err(OdinGoesRError::NoObjectError(String::from("No objects for GOES-R product and datetime initialization")))
        }
    }
    
    pub async fn download_updates(&mut self) -> Result<GoesRHotSpots> {
        //downloads latest file
        let product = &self.products[0];
        let last_object = if let Some(l_obj) = self.latest_objs.get(&product.name) {
            Some(l_obj)
        } else { 
            None
        };
        //download_most_recent_object(&self.s3_client, Utc::now(), &self.products[0], &self.sat_id, &self.data_dir).await?;
        let dt = Utc::now();
        let prefix = format!("{}/{}/{:03}/{:02}/", product.name, dt.year(), dt.ordinal(), dt.hour()); // https://stackoverflow.com/questions/76651472/do-rust-s3-sdk-datetimes-work-with-chrono
        let destination = PathBuf::from(&self.data_dir);
        let object = get_most_recent_object(&self.s3_client, &get_bucket(&self.sat_id), &prefix, last_object).await?;
        if let Some(obj) = object {
            self.latest_objs.insert(product.name.clone(), obj.clone());
            let data = get_goesr_data(&self.s3_client, obj, &destination, &product, self.sat_id.clone()).await?;
            let hotspots = read_goesr_data(&data)?;
            Ok(hotspots)
        } else {
            // try previous hour for case when we start the program before the data is up for the current hour (e.g., start at 5:00p - get error of no objects)
            let last_hour_object = get_last_hour_objects(&self.s3_client, &dt, &get_bucket(&self.sat_id), &product, last_object).await?;
            if let Some(obj) = last_hour_object {
                self.latest_objs.insert(product.name.clone(), obj.clone());
                let data = get_goesr_data(&self.s3_client, obj, &destination, &product, self.sat_id.clone()).await?;
                let hotspots = read_goesr_data(&data)?;
                Ok(hotspots)
            } else {
                Err(OdinGoesRError::NoObjectError(String::from("No objects for GOES-R product and datetime")))
            }
        }      
    }
    async fn sleep_for_remainder_of_cycle(&self) {
        sleep(minutes(5)).await;
    }
    pub async fn spawn_data_acquitision_task(&mut self) -> Result<()>{
        match  self.initial_download().await {
            Ok(init_hotspots) => {
                self.hself.send_msg( Initialize(init_hotspots) ).await?;
            }
            Err(e) => {
                error!("failed to download initial GOES-R data: {e:?}")
            }
        }
        loop {
            self.sleep_for_remainder_of_cycle().await;
            match  self.download_updates().await {
                Ok(hotspots) => {
                    self.hself.send_msg( Update(hotspots) ).await?;
                }
                Err(e) => {
                    error!("failed to download updated GOES-R data: {e:?}")
                }
            }
        }
    }
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
            init_objs.extend(object_vec[object_vec.len()-num_obj .. object_vec.len()].to_vec());//.into_iter().map(|x| init_objs.push(x));
        }
        Ok(Some(init_objs))
    } else {
        Ok(None)
    }
}

pub async fn get_multiple_last_hour_objects (client: &Client, dt: &DateTime<Utc>, bucket:&String, product:&GoesRProduct,  num_obj: &usize) -> Result<Option<Vec<Object>>> {
    let mut prev_hour = (dt.hour() as i16 -1 as i16) as i16;
    let mut day = dt.ordinal();
    if prev_hour<0 {
        prev_hour = 23;
        day = day-1;
    }
    let prefix_last_hour  = format!("{}/{}/{:03}/{:02}/", product.name, dt.year(), day, prev_hour); // https://stackoverflow.com/questions/76651472/do-rust-s3-sdk-datetimes-work-with-chrono
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
    let mut prev_hour = (dt.hour() as i16 -1 as i16) as i16;
    let mut prev_date = dt.ordinal();
    if prev_hour<0 { // update to read the previous days objects
        prev_hour = 23;
        prev_date = prev_date -1;
    }
    let prefix_last_hour  = format!("{}/{}/{:03}/{:02}/", product.name, dt.year(), prev_date, prev_hour); 
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
    let start = Instant::now();
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
