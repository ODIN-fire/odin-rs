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

use std::{collections::{VecDeque,HashMap}, path::{Path,PathBuf}, sync::Arc, time::Duration};
use odin_hrrr::HrrrDataSetRequest;
use serde::{Serialize,Deserialize};
use chrono::{DateTime,Utc};
use odin_build::define_load_config;
use odin_common::{geo::GeoRect};

//mod fetchdem;
pub mod actor;
pub mod errors;

define_load_config!{}

#[derive(Serialize,Deserialize,Debug)]
pub struct WindNinjaConfig {
    max_age: Duration, // how long to keep cached data files
    max_forecasts: u32, // max number of forecasts to keep for each region (in ringbuffer)
    windninja_path: String, // pathname for windninja executable

    dem_url: String, // url for odin_dem server to use
    dem_res_x: f64, // dem pixel sizes in [m]
    dem_res_y: f64,

    hrrr_fields: Vec<String>,
    hrrr_levels: Vec<String>,
}

/// this is what we distribute as updates 
#[derive(Serialize,Deserialize,Debug)]
pub struct Forecast {
    pub region: Arc<String>,
    pub date: DateTime<Utc>,    // for which this simulation was computed
    pub step: u32,              // hours from HRRR base date (0 means latest HRRR data set - indicator for confidence)
    pub path: String            // pathname where to find generated output
}

/// all available forecasts for a region, plus tracking of clients 
pub struct ForecastRegion {
    pub region: Arc<String>,
    pub bbox: GeoRect,
    pub dem_path: PathBuf,      // pathname to respective DEM file
    pub hrrr_ds_request: Arc<HrrrDataSetRequest>,

    pub n_clients: u32,       // if this drops to 0 we stop computing forecasts for this region
    pub forecasts: VecDeque<Forecast> // this is a ringbuffer ordered by forecast date (note we only keep the most recent forecast for each hour)
}

impl ForecastRegion {
    pub fn new (region: Arc<String>, bbox: GeoRect, dem_path: PathBuf, hrrr_ds_request: Arc<HrrrDataSetRequest>)->Self {
        ForecastRegion {
            region,
            bbox,
            dem_path,
            hrrr_ds_request,
            n_clients: 1,
            forecasts: VecDeque::new()
        }
    }
}

/// this is the data store snapshots are based on
pub type ForecastStore = HashMap<Arc<String>,ForecastRegion>;


/*
fn get_pathname (bbox: &BoundingBox, dir: &str) -> PathBuf {
    let mut path = PathBuf::from(dir);
    path.push( format!("{:.3}_{:.3}_{:.3}_{:.3}.tif", bbox.west, bbox.south, bbox.east, bbox.north));
    path
}

fn retrieve_dem (bbox: &BoundingBox, dem_path: &str, warp_path: &str, vrt_path: &str) -> Result<(),String> {
    to_utm_box(bbox).and_then( |bb| {
        match Command::new(warp_path)
        .arg("-t_srs")
        .arg(format!("+proj=utm +zone={} +datum=WGS84 +units=m", bb.zone))
        .arg("-te")
        .arg(format!("{:.3}", bb.west))
        .arg(format!("{:.3}", bb.south))
        .arg(format!("{:.3}", bb.east))
        .arg(format!("{:.3}", bb.north))
        .arg("-co")
        .arg("COMPRESS=DEFLATE")
        .arg("-co")
        .arg("PREDICTOR=2")
        .arg(vrt_path)
        .arg(dem_path)
        .spawn() {
            Ok(_) => Ok(()),
            Err(e) => Err(e.to_string())
        }
    })
}
*/