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

use std::{collections::{VecDeque,HashMap,HashSet}, path::{Path,PathBuf}, sync::Arc, time::Duration, net::SocketAddr};
use axum::Json;
use odin_hrrr::HrrrDataSetRequest;
use serde::{Serialize,Deserialize};
use chrono::{DateTime,Utc, Datelike, Timelike};
use odin_build::{define_load_asset, define_load_config, pkg_cache_dir};
use odin_common::{
    collections::RingDeque, datetime, fs::replace_env_var_path, geo::GeoRect, json_writer::{JsonWritable,JsonWriter}, utm::UtmRect, BoundingBox
};
use odin_dem::DemSource;
use lazy_static::lazy_static;

//mod fetchdem;
pub mod actor;
pub mod errors;
pub mod wind_service;


lazy_static! {
    pub static ref PKG_CACHE_DIR: PathBuf = pkg_cache_dir!();
    pub static ref WX_HRRR: Arc<String> = Arc::new( "hrrr".to_string());
}

define_load_config!{}
define_load_asset!{}

#[derive(Serialize,Deserialize,Debug)]
pub struct WindConfig {
    max_age: Duration, // how long to keep cached data files
    max_forecasts: usize, // max number of forecasts to keep for each region (in ringbuffer)
    windninja_cmd: String, // pathname for windninja executable
    mesh_res: f64, // WindNinja mesh resolution in meters
    wind_height: f64, // above ground in meters

    huvw_csv_grid_cmd: String, // where to find the HUVW CSV file generator
    huvw_csv_vector_cmd: String, // where to find the HUVW CSV vector generator
    huvw_json_contour_cmd: String, // where to find the GeoJSON contour generator
    hrrr_csv_grid_cmd: String, // the direct HRRR to CSV grid generator

    dem: DemSource, // where to get the DEM grid from
    dem_res: f64, // dem pixel sizes in meters

    // the fields and levels we need from HRRR
    hrrr_fields: Vec<String>,
    hrrr_levels: Vec<String>,
}

/// the internal data structure that represents the input data for a single WindNinja run
/// this is an aggregate of all the data we need to feed into WindNinja. It currently has a lot of overlap with Forecast (which is
/// supposed to capture the *result* of a WindNinja run) but that might change. Since we turn WnJobs into Forecasts the overlap is acceptable 
#[derive(Debug)]
struct WnJob {
    region: Arc<String>, // our region name
    date: DateTime<Utc>, // the hour for which this simulation is (base + step)
    step: usize, // informal - the wx forecast steo (hourly distance to base forecast)
    mesh_res: f64, // in meters
    wind_height: f64, // above ground in meters
    wx_src: Arc<String>,
    wx_path: Arc<PathBuf>, // WindNinja wx input (HRRR)
    dem_path: Arc<PathBuf>, // WindNinja DEM input
    wn_out_basename: Arc<String>
}


/// NOTE - the wn_out_base_name has to be kept in sync with WindNinja
impl From<WnJob> for Forecast {
    fn from (wn_job: WnJob) -> Self { // this consumes the WnJob so no need to clone
        Forecast {
            region: wn_job.region,
            date: wn_job.date,
            step: wn_job.step,
            mesh_res: wn_job.mesh_res,
            wind_height: wn_job.wind_height,
            wx_src: wn_job.wx_src,
            wx_path: wn_job.wx_path,
            dem_path: wn_job.dem_path,
            wn_out_base_name: wn_job.wn_out_basename,
        }
    }
}

/// aggregate with the results of a single WindNinja run
/// this is what we distribute as updates so it has to clone efficiently (use ARCs)
#[derive(Debug,Clone)]
pub struct Forecast {
    // what we get from the WnJob
    pub region: Arc<String>,

    pub date: DateTime<Utc>,    // for which this simulation was computed
    pub step: usize,            // hours from forecast (HRRR) base date (0 means latest HRRR data set - indicator for confidence)
    
    pub mesh_res: f64,          // WindNinja mesh resolution in meters
    pub wind_height: f64,       // of WindNinja computed values - above ground in meters
    pub wx_src: Arc<String>,    // e.g. "HRRR"
    pub wx_path: Arc<PathBuf>,  // the HRRR data this forecast is based on
    pub dem_path: Arc<PathBuf>, // the DEM data this forecast is based on

    // the primary WindNinja output file basename (huvw UTM grid). All other filenames (WGS84 grid/vec and contour) derived from here
    pub wn_out_base_name: Arc<String>, // this does *not* include the extension
    // TODO - add HRRR-based grid/vector/contour base_name (with 3000m resolution)
}

impl Forecast {
    pub fn get_huvw_utm_grid_path (&self)->PathBuf {
        pkg_cache_dir!().join( format!("{}.tif", self.wn_out_base_name))
    }

    pub fn get_huvw0_utm_grid_path (&self)->PathBuf {
        pkg_cache_dir!().join( format!("{}_0.tif", self.wn_out_base_name))
    }

    pub fn get_huvw_grid_path (&self)->PathBuf {
        pkg_cache_dir!().join( format!("{}.csv.gz", self.wn_out_base_name))
    }

    pub fn get_huvw_vector_path (&self)->PathBuf {
        pkg_cache_dir!().join( format!("{}_vector.csv.gz", self.wn_out_base_name))
    }

    pub fn get_huvw_contour_path (&self)->PathBuf {
        pkg_cache_dir!().join( format!("{}_contour.json", self.wn_out_base_name))
    }

    pub fn get_hrrr_10_grid_path (&self)->PathBuf {
        pkg_cache_dir!().join( format!("{}_hrrr_10.csv.gz", self.wn_out_base_name))
    }

    pub fn get_hrrr_80_grid_path (&self)->PathBuf {
        pkg_cache_dir!().join( format!("{}_hrrr_80.csv.gz", self.wn_out_base_name))
    }

    // TODO - add grid/contour for HRRR (3km resolution)

    pub fn to_json (&self)->String {
        let mut w = JsonWriter::with_capacity(512);
        w.write_object(|w| {
            w.write_field("region", self.region.as_str());
            w.write_field("date", datetime::to_epoch_millis(self.date));
            w.write_field("step", self.step as u64);
            w.write_field("mesh", self.mesh_res);
            w.write_field("wxSrc", self.wx_src.as_ref());
            w.write_field("urlBase", self.wn_out_base_name.as_str());
        });
        w.to_string()
    }

    pub fn write_partial_json_to (&self, w: &mut JsonWriter) {
         w.write_object(|w| {
            w.write_field("date", datetime::to_epoch_millis(self.date));
            w.write_field("step", self.step as u64);
            w.write_field("mesh", self.mesh_res);
            w.write_field("wxSrc", self.wx_src.as_ref());
            w.write_field("urlBase", self.wn_out_base_name.as_str());
        })
    }
}

/// all available forecasts for a region, plus tracking of clients 
pub struct ForecastRegion {
    pub region: Arc<String>,
    pub bbox: GeoRect,
    pub utm_rect: UtmRect,
    pub dem_path: Arc<PathBuf>,      // pathname to respective DEM file
    pub hrrr_ds_request: Arc<HrrrDataSetRequest>,

    pub n_clients: usize,       // if this drops to 0 we stop computing forecasts for this region
    pub client_addrs: HashSet<SocketAddr>,

    pub forecasts: VecDeque<Forecast> // this is a ringbuffer ordered by forecast date (note we only keep the most recent forecast for each hour)
}

impl ForecastRegion {
    pub fn new (region: Arc<String>, bbox: GeoRect, utm_rect: UtmRect, dem_path: Arc<PathBuf>, hrrr_ds_request: Arc<HrrrDataSetRequest>, max_steps: usize)->Self {
        ForecastRegion {
            region,
            bbox,
            utm_rect,
            dem_path,
            hrrr_ds_request,
            n_clients: 0,
            client_addrs: HashSet::new(),
            forecasts: VecDeque::with_capacity( max_steps)
        }
    }

    pub fn add_client (&mut self, remote_addr: Option<SocketAddr>) {
        if let Some(addr) = remote_addr {
            let len0 = self.client_addrs.len();
            self.client_addrs.insert( addr);
            if self.client_addrs.len() > len0 {
                self.n_clients += 1;
            }
        } else {
            self.n_clients +=1;
        }
    }

    pub fn remove_client (&mut self, remote_addr: &Option<SocketAddr>)->bool {
        if let Some(addr) = remote_addr {
            let len0 = self.client_addrs.len();
            self.client_addrs.remove( addr);
            if self.client_addrs.len() < len0 {
                self.n_clients -= 1;
                true
            } else {
                false
            }
        } else {
            self.n_clients -=1;
            true
        }
    }
}

impl JsonWritable for ForecastRegion {
    fn write_json_to (&self, w: &mut JsonWriter) {
        w.write_object(|w| {
            w.write_field("region", self.region.as_str());
            w.write_json_field("bbox", &self.bbox);
            w.write_array_field("forecasts", |w| {
                for f in &self.forecasts {
                    f.write_partial_json_to(w);
                }
            });
        })
    }
}

/// this is the data store snapshots are based on
pub type ForecastStore = HashMap<Arc<String>,ForecastRegion>;

pub fn forecast_regions_to_json (fcs: &ForecastStore)->String {
    let mut  w = JsonWriter::with_capacity( fcs.len() * 1024);
    w.write_object(|w| {
        w.write_array_field("regions", |w| {
            for fcr in fcs.values() {
                fcr.write_json_to(w);
            }
        });
    });

    w.to_string()
}
