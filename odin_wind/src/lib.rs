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

use std::{
    collections::{HashMap, HashSet, VecDeque}, 
    fs::File, io::{BufWriter,Write}, net::SocketAddr, 
    path::{Path,PathBuf}, sync::Arc, time::Duration
};
use axum::Json;
use odin_hrrr::HrrrDataSetRequest;
use serde::{Serialize,Deserialize};
use chrono::{DateTime,Utc, Datelike, Timelike};
use odin_build::{define_load_asset, define_load_config, pkg_cache_dir};
use odin_common::{
    cartesian3::Cartesian3, cartographic::Cartographic, collections::RingDeque, datetime, 
    fs::{path_str_to_fname, replace_env_var_path, replace_filename}, geo::GeoRect, 
    json_writer::{JsonWritable,JsonWriter}, push_all_str, 
    ron::{TypedCompactRon, from_typed_compact_ron, to_typed_compact_ron}, 
    sqrt, strings::extract_json_payload_object, utm::UtmRect, BoundingBox
};
use odin_action::DynDataRefAction;
use odin_dem::DemSource;
use lazy_static::lazy_static;
use odin_gdal::{{gdal::{Dataset,raster::RasterBand}}, contour::ContourBuilder, read_row};

//mod fetchdem;
pub mod actor;
pub mod errors;
use errors::Result;

use crate::errors::op_failed;
pub mod wind_service;

pub mod server;
pub mod server_client;

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

    dem: DemSource, // where to get the DEM grid from
    dem_res: f64, // dem pixel sizes in meters

    // the fields and levels we need from HRRR
    hrrr_fields: Vec<String>,
    hrrr_levels: Vec<String>,
}

#[derive(Debug,Clone,Serialize,Deserialize)] 
pub struct WindRegion {
    pub name: String,
    pub bbox: GeoRect,
}

impl WindRegion {
    pub fn new (name: impl ToString, bbox: GeoRect)->Self {
        WindRegion { name: name.to_string(), bbox }
    }
}

#[derive(Debug,Serialize,Deserialize)]
pub struct AddWindClient {
    pub wn_region: WindRegion,
    pub remote_addr: SocketAddr
}

impl TypedCompactRon<'_> for AddWindClient {} 

#[derive(Debug)]
pub enum SubscribeResponse {
    Add(AddWindClientResponse),
    Remove(RemoveWindClientResponse)
}

/// the response to a AddWindClient message. This is fed into the subscribe_action
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct AddWindClientResponse {
    pub wn_region: WindRegion,
    pub is_new: bool,
    pub rejection: Option<String>, // if None then client request was accepted (but region might already be monitored)

    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_addr: Option<SocketAddr> // only present when sent internally
}

impl TypedCompactRon<'_> for AddWindClientResponse {}

#[derive(Debug,Serialize,Deserialize)] 
pub struct RemoveWindClient {
    pub region: Option<String>, // of region
    pub remote_addr: SocketAddr
}

impl TypedCompactRon<'_> for RemoveWindClient {}

#[derive(Debug,Serialize,Deserialize)]
pub struct RemoveWindClientResponse {
    pub region: String,
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

impl WnJob {
    // WindNinja has a different naming convention: BigSur_07-10-2025_1900_150m_huvw.tif
    pub fn get_wn_filename (&self)->String {
        let d = &self.date;
        format!("{}_{:02}-{:02}-{:4}_{:2}{:2}_{:.0}m_huvw.tif", 
            path_str_to_fname( self.region.as_str()), d.month(), d.day(), d.year(), d.hour(), d.minute(), self.mesh_res)
    }

    pub fn get_wn_path (&self, suffix: &str) -> PathBuf {
        let mut filename = self.wn_out_basename.as_ref().clone();
        filename.push_str(suffix);
        pkg_cache_dir!().join( filename)
    } 

    pub fn output_files_exist (&self)->bool {
        let mut filename = self.wn_out_basename.as_ref().clone();
        let l = filename.len();

        push_all_str!( filename, huvw_grid_suffix(), ".gz");
        let mut path = pkg_cache_dir!().join( &filename);
        if !path.is_file() { return false } 

        filename.truncate(l);
        push_all_str!( filename, huvw_vector_suffix(),".gz");
        replace_filename( &mut path, &filename);
        if !path.is_file() { return false } 

        filename.truncate(l);
        push_all_str!( filename, huvw_contour_suffix(),".gz");
        replace_filename( &mut path, &filename);
        if !path.is_file() { return false }   

        true
    }
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
#[derive(Debug,Clone,Serialize,Deserialize)]
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
    pub wn_out_base_name: Arc<String>, // this does *not* include the extension as we use it as the base for several products
}

impl TypedCompactRon<'_> for Forecast {}

//--- the various paths of WindNinja computed products we derive from wn_out_base_name

pub fn get_tmp_path (wn_out_base_name: &str) -> PathBuf { pkg_cache_dir!().join( format!("{}_.tif", wn_out_base_name)) }

// the WindNinja huvw grid based product suffixes
pub fn huvw_wgs84_suffix ()->&'static str { "__wgs84.tif" }
pub fn huvw_grid_suffix ()->&'static str { "__grid.csv" }
pub fn huvw_vector_suffix ()->&'static str { "__vector.csv" }
pub fn huvw_contour_suffix ()->&'static str { "__contour.json" }

// the HRRR based product suffixes
pub fn hrrr_wgs84_suffix ()->&'static str { "__hrrr__wgs84.tif" }

pub fn hrrr_10_grid_suffix ()->&'static str { "__hrrr__10__grid.csv" }
pub fn hrrr_10_vector_suffix ()->&'static str { "__hrrr__10__vector.csv" }
pub fn hrrr_10_contour_suffix ()->&'static str { "__hrrr__10__contour.json" }

pub fn hrrr_80_grid_suffix ()->&'static str { "__hrrr__80__grid.csv" }
pub fn hrrr_80_vector_suffix ()->&'static str { "__hrrr__80__vector.csv" }
pub fn hrrr_80_contour_suffix ()->&'static str { "__hrrr__80__contour.json" }

impl Forecast {

    /// the WindNinja output filename
    /// Note that WindNinja has a different naming convention: BigSur_07-10-2025_1900_150m_huvw.tif
    /// (it does not capture the forecast step, i.e. we could overwrite a more actual forecast)
    pub fn get_wn_output_path (&self) -> PathBuf {
        let d = &self.date;
        let fname = format!("{}_{:02}-{:02}-{:4}_{:02}{:02}_{:.0}m_huvw.tif", 
            path_str_to_fname( self.region.as_str()), d.month(), d.day(), d.year(), d.hour(), d.minute(), self.mesh_res);
        pkg_cache_dir!().join( fname)
    }

    pub fn get_wn_path (&self, suffix: &str) -> PathBuf {
        let mut filename = self.wn_out_base_name.as_ref().clone();
        filename.push_str(suffix);
        pkg_cache_dir!().join( filename)
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


/// data we need before we can create WnJobs. This is only kept in the WindActor
pub struct WnJobRegion {
    pub region: Arc<String>,
    pub utm_rect: UtmRect,           // the (approximated) region bbox in UTM
    pub dem_path: Arc<PathBuf>,      // pathname to respective DEM file
    pub hrrr_ds_request: Arc<HrrrDataSetRequest>,
}

/// all available forecasts for a region, plus the respective clients for that region. This is where we store Forecast results
pub struct ForecastRegion {
    pub region: Arc<String>,
    pub bbox: GeoRect,
    pub client_addrs: HashSet<SocketAddr>,
    pub forecasts: VecDeque<Forecast> // this is a ringbuffer ordered by forecast date (note we only keep the most recent forecast for each hour)
}

impl ForecastRegion {
    pub fn new (region: Arc<String>, bbox: GeoRect, max_steps: usize)->Self {
        ForecastRegion {
            region,
            bbox,
            client_addrs: HashSet::new(),
            forecasts: VecDeque::with_capacity( max_steps)
        }
    }

    pub fn add_client (&mut self, remote_addr: SocketAddr) {
        self.client_addrs.insert( remote_addr);
    }

    pub fn remove_client (&mut self, remote_addr: &SocketAddr)->bool {
        self.client_addrs.remove( remote_addr)
    }

    pub fn add_forecast (&mut self, forecast: Forecast)->Option<&Forecast> {
        let mut fcs = &mut self.forecasts;
        for i in 0..fcs.len() {
            let f = &fcs[i];
            if f.date == forecast.date  { 
                if forecast.step < f.step {  // this replaces an older, now obsolete forecast for the same hour
                    fcs[i] = forecast;
                    return Some( &fcs[i])

                } else {
                    println!("ignoring dead-on-arrival forecast {:?}", forecast);
                    return None
                }
            } else if f.date > forecast.date { 
                println!("inserting previously missing forecast {:?}", forecast);
                fcs.insert_into_ringbuffer(i, forecast);
                return Some(&fcs[i])
            }
        }
        // if we get here we append (and possibly drop the first forecast)
        fcs.push_to_ringbuffer( forecast);
        fcs.back()
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

/// message to request action execution with the current HotspotStore
#[derive(Debug)] 
pub struct ExecSnapshotAction(pub DynDataRefAction<ForecastStore>);

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

pub fn write_huvw_csv_grid<P> (ds: &Dataset, path: P, bands: &[usize])->Result<()> where P: AsRef<Path> {
    if bands.len() < 3 { return Err( errors::OdinWindError::OpFailedError("not enough bands for huvw grid".into())) }

    let (cols,rows) = ds.raster_size();
    let a = ds.geo_transform()?;

    let x0 = a[0];
    let cx = a[1];
    let y0 = a[3];
    let cy = a[5];

    let h_band = ds.rasterband(bands[0])?;
    let u_band = ds.rasterband(bands[1])?;
    let v_band = ds.rasterband(bands[2])?;

    // the w band is optional (we might only have horizontal wind components in the input dataset)
    let w_band = if bands.len() > 3 { Some(ds.rasterband(3)?) } else { None };

    let mut h_line: Vec<f32> = vec![0.0; cols];
    let mut u_line: Vec<f32> = vec![0.0; cols];
    let mut v_line: Vec<f32> = vec![0.0; cols];
    let mut w_line: Vec<f32> = vec![0.0; cols];

    w_line[0] = 0.1; // FIXME - something in the shaders breaks if there is only one w value

    let mut file = File::create(path)?;
    let mut buf = BufWriter::new( file);

    write!( buf, "# nx:{}, x0:{}, dx:{}, ny:{}, y0:{}, dy:{}\n", cols, x0, cx, rows, y0, cy);
    write!( buf, "h, u, v, w, spd\n");

    for j in 0..rows {
        read_row( &h_band, j as isize, h_line.as_mut_slice())?;
        read_row( &u_band, j as isize, u_line.as_mut_slice())?;
        read_row( &v_band, j as isize, v_line.as_mut_slice())?;

        if let Some(w_band) = &w_band { read_row( w_band, j as isize, w_line.as_mut_slice())?; }

        for i in 0..cols {
            let h = h_line[i];
            let u = u_line[i];
            let v = v_line[i];
            let w = w_line[i];
            let spd = (u*u + v*v + w*w).sqrt();
            write!( buf, "{:.1},{:.1},{:.1},{:.1},{:.1}\n", h, u, v, w, spd);
        }
    }

    buf.flush()?;
    Ok(())
}

/// note that vector origins are in ECEF and the length is relative (to the cell size) for display purposes.
/// Note also that the input dataset has to be a WGS84 grid and x- and y- resolution (cell size) should be the same
pub fn write_huvw_csv_cell_vectors<P> (ds: &Dataset, path: P, mesh_res: f64, bands: &[usize])->Result<()> where P: AsRef<Path> {
    if bands.len() < 3 { return Err( errors::OdinWindError::OpFailedError("not enough bands for huvw grid".into())) }

    // somewhat arbitrary - we might have a color coded version with fixed lengths in the future
    fn cell_scale_factor (spd: f64) ->f64 {
        if spd < 2.2352      { 0.2 }  // < 5 mph
        else if spd < 4.4704 { 0.4 }  // < 10 mph
        else if spd < 8.9408 { 0.6 }  // < 20 mph
        else                 { 0.8 }  // >= 20 mph
    }

    let (cols,rows) = ds.raster_size();
    let a = ds.geo_transform()?;

    let x0 = a[0];
    let cx = a[1];
    let y0 = a[3];
    let cy = a[5];
    let cx2 = cx / 2.0;
    let cy2 = cy / 2.0;

    let h_band = ds.rasterband(bands[0])?;
    let u_band = ds.rasterband(bands[1])?;
    let v_band = ds.rasterband(bands[2])?;

    // the w band is optional (we might only have horizontal wind components in the input dataset)
    let w_band = if bands.len() > 3 { Some(ds.rasterband(3)?) } else { None };

    let mut h_line: Vec<f32> = vec![0.0; cols];
    let mut u_line: Vec<f32> = vec![0.0; cols];
    let mut v_line: Vec<f32> = vec![0.0; cols];
    let mut w_line: Vec<f32> = vec![0.0; cols];

    let mut file = File::create(path)?;
    let mut buf = BufWriter::new( file);

    write!( buf, "# length:{}\n", cols*rows);
    write!( buf, "x0, y0, z0, x1, y1, z1, spd\n");

    for j in 0..rows {
        read_row( &h_band, j as isize, h_line.as_mut_slice())?;
        read_row( &u_band, j as isize, u_line.as_mut_slice())?;
        read_row( &v_band, j as isize, v_line.as_mut_slice())?;

        if let Some(w_band) = &w_band { read_row( w_band, j as isize, w_line.as_mut_slice())?; }

        for i in 0..cols {
            let h = h_line[i] as f64;  // TODO - does this include wind height or do we have to add it explicitly?
            let u = u_line[i] as f64;
            let v = v_line[i] as f64;
            let w = w_line[i] as f64;

            let spd = sqrt(u*u + v*v + w*w);

            // the grid values are for the respective grid cell centers. There is no rotation
            let lon_deg = x0 + (cx * i as f64) + cx2; // grid point longitude (degrees)
            let lat_deg = y0 + (cy * j as f64) + cy2; // grid point latitude (degrees)
            let cp = Cartographic::from_degrees( lon_deg, lat_deg, h);
            let p: Cartesian3 = cp.into();

            let s = cell_scale_factor(spd) * mesh_res;  // length of display vector in [m]
            let f = s / spd;

            let su = u * f;
            let sv = v * f;
            let sw = w * f;

            // since cell size is assumed to be > 100m we don't need decimals. These vectors are only for display
            write!( buf, "{:.0},{:.0},{:.0},{:.0},{:.0},{:.0},{:.1}\n", p.x, p.y, p.z, p.x+su, p.y+sv, p.z+sw, spd);
        }
    }

    buf.flush()?;
    Ok(())
}

pub fn write_windspeed_contour<P> (ds: &Dataset, path: P, band: usize) -> Result<()> where P: AsRef<Path> {
    if ds.raster_count() < 5 { return Err( errors::OdinWindError::OpFailedError("invalid input data set".into())) }

    let mut contourer = ContourBuilder::new( ds, path)?;

    contourer
        .set_band( band as i32)
        .set_interval(5)
        .set_poly()
        .set_attr_min_name("minSpeed")?
        .set_attr_max_name("maxSpeed")?
        .set_quiet()
        .exec()?;

    // compress here

    Ok(())
}
