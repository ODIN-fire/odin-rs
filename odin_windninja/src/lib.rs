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
use odin_common::{geo::GeoRect,collections::RingDeque};

//mod fetchdem;
pub mod actor;
pub mod errors;

define_load_config!{}

#[derive(Serialize,Deserialize,Debug)]
pub struct WindNinjaConfig {
    max_age: Duration, // how long to keep cached data files
    max_forecasts: usize, // max number of forecasts to keep for each region (in ringbuffer)
    windninja_path: String, // pathname for windninja executable
    mesh_res: f64, // WindNinja mesh resolution in meters
    wind_height: f64, // above ground in meters

    dem_url: String, // url for odin_dem server to use
    dem_res: f64, // dem pixel sizes in meters

    // the fields and levels we need from HRRR
    hrrr_fields: Vec<String>,
    hrrr_levels: Vec<String>,
}

/// this is what we distribute as updates 
#[derive(Serialize,Deserialize,Debug)]
pub struct Forecast {
    pub region: Arc<String>,
    pub date: DateTime<Utc>,    // for which this simulation was computed
    pub step: usize,            // hours from HRRR base date (0 means latest HRRR data set - indicator for confidence)
    pub path: Arc<PathBuf>  // pathname where to find generated output
}

/// all available forecasts for a region, plus tracking of clients 
pub struct ForecastRegion {
    pub region: Arc<String>,
    pub bbox: GeoRect,
    pub dem_path: Arc<PathBuf>,      // pathname to respective DEM file
    pub hrrr_ds_request: Arc<HrrrDataSetRequest>,

    pub n_clients: usize,       // if this drops to 0 we stop computing forecasts for this region
    pub forecasts: VecDeque<Forecast> // this is a ringbuffer ordered by forecast date (note we only keep the most recent forecast for each hour)
}

impl ForecastRegion {
    pub fn new (region: Arc<String>, bbox: GeoRect, dem_path: Arc<PathBuf>, hrrr_ds_request: Arc<HrrrDataSetRequest>, max_steps: usize)->Self {
        ForecastRegion {
            region,
            bbox,
            dem_path,
            hrrr_ds_request,
            n_clients: 1,
            forecasts: VecDeque::with_capacity( max_steps)
        }
    }
}

/// this is the data store snapshots are based on
pub type ForecastStore = HashMap<Arc<String>,ForecastRegion>;
