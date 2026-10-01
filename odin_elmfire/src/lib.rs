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

#![allow(unused)]

use std::{
    collections::HashMap,
    fs::{File, create_dir, create_dir_all, remove_dir_all, remove_file},
    io::{Read,Write}, ops::Range,
    path::{Path,PathBuf}, result::Result as StdResult, sync::Arc, time::Duration
};
use chrono::{DateTime,Utc,Timelike};
use serde::{Serialize,Deserialize};
use ron;
use lazy_static::lazy_static;
use regex::Regex;
use uom::si::{
    f64::{Angle, Area, Length, LinearPowerDensity, Mass, Power, Pressure, Ratio, ThermodynamicTemperature, Velocity, Volume},
};
use uom::si::{
    angle::{degree},
    thermodynamic_temperature::{degree_celsius,degree_fahrenheit},
    linear_power_density::{watt_per_meter},
    pressure::{pascal,inch_of_mercury},
    velocity::{mile_per_hour,meter_per_second,foot_per_minute},
    length::{kilometer,meter,foot},
    area::{acre,hectare,square_meter},
    volume::{acre_foot},
    mass::{milligram,kilogram,microgram},
    ratio::{percent,ratio},
    power::{megawatt},
};
use askama::Template;
use csv;
use gdal::Dataset;

use odin_build::{define_load_asset, define_load_config, pkg_cache_dir, pkg_data_dir};
use odin_macro::public_struct;
use odin_common::{
    BoundingBox, angle::Angle360, apply_index_margin, as_min_max,
    datetime::{self, as_minutes, full_hour, hours, short_utc_datetime_string, to_epoch_millis},
    fs::{ EnvPathBuf, ensure_writable_dir, filepath_contents_as_string, filestem, find_first_match_in_dir, matching_files_in_dir, path_as_str, set_filepath_contents},
    geo::{GeoPoint,GeoRect},
    json_writer::{JsonEscapedStr, JsonWritable, JsonWriter, NumFormat},
    run_captured_cmd,
    utm::{UtmPoint, UtmRect, geo_to_utm, try_geo_to_utm_zone},
};
use odin_gdal::{
    geods::GeoDs, get_mem_driver, get_raster_info, crop_mem_dataset, get_exterior_data_bounds, replace_rasterband_value,
    warp::{ResampleAlg, SimpleWarpBuilder}
};
use odin_openmeteo::get_fc_days;
use odin_landfire::{evt_cats::{EVT_CATEGORIES, EvtCategories}, get_evt_values_at_utm_points};
use odin_server::{spa::SpaService,ws_service::ws_msg_from_json};

use static_assertions::assert_impl_all;

pub mod input;
use input::{create_fixed_input, create_internal_input, create_landfire_input, create_wx_based_input};

pub mod actor;
pub mod service;
pub use service::ElmfireService;
pub mod fixed;

pub mod errors;
use errors::{OdinElmfireError,Result,op_failed};

use crate::fixed::parse_nml_array_line;

lazy_static! {
    pub static ref CACHE_DIR: PathBuf = { pkg_cache_dir!() };
    pub static ref DATA_DIR: PathBuf = { pkg_data_dir!() };

    pub static ref SPOTTING_DIR: PathBuf = DATA_DIR.join("spotting");
    pub static ref FIXED_DIR: PathBuf = DATA_DIR.join("fixed");
    pub static ref NML_RE: Regex = Regex::new( r#"^[^._].*\.nml"#).unwrap(); // namelist file pattern

    pub static ref FLIN_RE: Regex = Regex::new( r#"flin_.*\.tif"#).unwrap();
    pub static ref TOA_RE: Regex = Regex::new( r#"time_of_arrival_.*\.tif"#).unwrap();
    pub static ref VS_RE: Regex = Regex::new( r#"vs_.*\.tif"#).unwrap();
    pub static ref CF_RE: Regex = Regex::new( r#"crown_fire_.*\.tif"#).unwrap();
    pub static ref FL_RE: Regex = Regex::new( r#"flame_length_.*\.tif"#).unwrap();
    pub static ref AREA_RE: Regex = Regex::new( r#"acres_.*\.csv"#).unwrap();
    pub static ref STATS_RE: Regex = Regex::new( r#"fire_size_stats.*\.csv"#).unwrap();

    pub static ref EF_RUN_RE: Regex = Regex::new( r#"elmfire.ron"#).unwrap();
}

const WX_DT: Duration = Duration::from_secs( 60 * 15);

define_load_config!{}
define_load_asset!{}

#[derive(Deserialize,Debug)]
#[public_struct]
struct ElmfireConfig {
    lf_src_name: String,       // filename of odin_landfire GeoDs source to use (->odin_gdalgdal::GeoDs)
    lf_res_m: u32,             // landfire pixel resolution in meters

    nfdrs_config_name: String,
    nfdrs_past_days: u32,     // number of past days to retrieve to compute (accumulated) fuel moisture (>100h)

    wx_config_name: String,    // name of odin_openmeteo config file to use
    wx_res_m: u32,             // pixel resolution in meter for generated wx data sets (note this does not have to be the native wx_model resolution)
    wx_model: String,

    bbox_size_m: u32,         // edge size of UTM bounding box in meter (should be even multiple of wx_res_m)

    wind_config_name: String, // WindNinja config name - used if we request micro grid wind instead of wx forecast
    wind_res_m: u32,          // should be multiple of lf_res_m (we specify it here separately)

    input_dt: Duration,       // meteorologicel time step length
    small_fire_cells: usize,  // number of cells for final perimeter we consider as small fire (needs post-processing to avoid pixelized contour artifacts)

    elmfire_cmd: EnvPathBuf   // the Elmfire executable
}

/// structure with given wx and fuel data values for the same time point.
/// Used to construct time/location dependent Elmfire inputs for wind and fuel moisture
/// We need this
#[derive(Debug,Clone, Serialize, Deserialize)]
#[public_struct]
struct FixedInputStep {
    ws: Velocity,
    wd: Angle360,
    m1: Ratio,
    m10: Ratio,
    m100: Ratio,
    lh: Ratio,
    lw: Ratio
}

/// parse input with NML (Fortran Name List) formatted key=val,.. entries
pub fn parse_fixed_wx_lines (src: &str, n_hours: usize)->Result<Vec<FixedInputStep>> {
    let mut ws: Vec<f64> = Vec::with_capacity(0);
    let mut wd: Vec<f64> = Vec::with_capacity(0);
    let mut m1: Vec<f64> = Vec::with_capacity(0);
    let mut m10: Vec<f64> = Vec::with_capacity(0);
    let mut m100: Vec<f64> = Vec::with_capacity(0);
    let mut lh: Vec<f64> = Vec::with_capacity(0);
    let mut lw: Vec<f64> = Vec::with_capacity(0);

    for line in src.lines() {
        if let Some( (k,vs) ) = parse_nml_array_line(line).map_err( |e| op_failed!("invalid input line: {}", e))? {
            match k.as_str() {
                "ws" => ws = vs,
                "wd" => wd = vs,
                "m1" => m1 = vs,
                "m10" => m10 = vs,
                "m100" => m100 = vs,
                "lh" => lh = vs,
                "lw" => lw = vs,
                o => return Err( op_failed!("unknown wx variable '{}'", o))
            }
        }
    }

    get_fixed_steps( n_hours, &ws, &wd, &m1, &m10, &m100, &lh, &lw)
}

pub fn fixed_src_from_steps (steps: &[FixedInputStep])->String {
    let ws = steps.iter().map( |s| format!("{:.1}",s.ws.get::<mile_per_hour>())).collect::<Vec<String>>().join(",");
    let wd = steps.iter().map( |s| format!("{:.0}", s.wd.degrees())).collect::<Vec<String>>().join(",");
    let m1 = steps.iter().map( |s| format!("{:.0}", s.m1.get::<percent>())).collect::<Vec<String>>().join(",");
    let m10 = steps.iter().map( |s| format!("{:.0}", s.m10.get::<percent>())).collect::<Vec<String>>().join(",");
    let m100 = steps.iter().map( |s| format!("{:.0}", s.m100.get::<percent>())).collect::<Vec<String>>().join(",");
    let lh = steps.iter().map( |s| format!("{:.0}", s.lh.get::<percent>())).collect::<Vec<String>>().join(",");
    let lw = steps.iter().map( |s| format!("{:.0}", s.lw.get::<percent>())).collect::<Vec<String>>().join(",");

    format!("ws={ws}\nwd={wd}\nm1={m1}\nm10={m10}\nm100={m100}\nlh={lh}\nlw={lw}\n")
}

/// turn lists of primitive input values into a vec of FxiedInputSteps. All provided input value slices have to be non-empty.
/// If an input value slice has not enough elements for a time step it is filled up with the last element
pub fn get_fixed_steps (n_steps: usize, ws_mph: &[f64], wd_deg: &[f64], m1: &[f64], m10: &[f64], m100: &[f64], lh: &[f64], lw: &[f64])->Result<Vec<FixedInputStep>> {
    if ws_mph.is_empty() || wd_deg.is_empty() || m1.is_empty() || m10.is_empty() || m100.is_empty() || lh.is_empty() || lw.is_empty(){
        return Err( op_failed!("missing FixedInputStep component"))
    }

    let mut vs: Vec<FixedInputStep> = Vec::with_capacity( n_steps);

    let ws_len = ws_mph.len();
    let wd_len = wd_deg.len();
    let m1_len = m1.len();
    let m10_len = m10.len();
    let m100_len = m100.len();
    let lh_len = lh.len();
    let lw_len = lw.len();

    for i in 0..n_steps {
        let ws = Velocity::new::<mile_per_hour>( if i < ws_len { ws_mph[i] } else { ws_mph[ws_len-1] });
        let wd = Angle360::from_degrees( if i < wd_len { wd_deg[i] } else { wd_deg[wd_len-1] });
        let m1 = Ratio::new::<percent>( if i < m1_len { m1[i] } else { m1[m1_len-1] });
        let m10 = Ratio::new::<percent>( if i < m10_len { m10[i] } else { m10[m10_len-1] });
        let m100 = Ratio::new::<percent>( if i < m100_len { m100[i] } else { m100[m100_len-1] });
        let lh = Ratio::new::<percent>( if i < lh_len { lh[i] } else { lh[lh_len-1] });
        let lw = Ratio::new::<percent>( if i < lw_len { lw[i] } else { lw[lw_len-1] });

        vs.push( FixedInputStep{ws,wd,m1,m10,m100,lh,lw});
    }

    Ok( vs )
}


/// this represents the translated outcome of a Elmfire run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElmfireOutput {
    pub stats: Vec<FireStats>, // at the end of the simulation
    pub area: Vec<AreaSnapshot>,
    pub comp_stats: ComputedStats,
    pub files: Vec<String>, // filenames of generated images and geojsons
    pub geo_rect: GeoRect // wgs84 bounding box of files (if any)
}

assert_impl_all!( ElmfireOutput : Send,Sync);

/// those we read from the Elmfire generated acres*.csv file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AreaSnapshot {
    pub date: DateTime<Utc>,
    pub area: Area
}

impl JsonWritable for AreaSnapshot {
    fn write_json_to (&self, w: &mut JsonWriter) {
        w.write_object(|w| {
            w.write_date_field("date", self.date);
            w.write_f64_field("area", self.area.get::<acre>(), NumFormat::Fp1);
        });
    }
}

/// those are read from the Elmfire generated fire_size_stats.csv
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FireStats {
    pub date: DateTime<Utc>,
    pub total_area: Area,
    pub crown_fire_area: Area,
    pub volume: Volume,
    pub population: f64,        // ?? unit
    pub real_estate_value: f64, // ?? unit
    pub land_value: f64,        // ?? unit
    pub n_embers: u32,
    pub pm25_release: Mass,
    pub hrr_peak: Power,         // heat release rate
}

impl JsonWritable for FireStats {
    fn write_json_to (&self, w: &mut JsonWriter) {
        w.write_object(|w|{
            w.write_date_field("date", self.date);
            w.write_f64_field("totalArea", self.total_area.get::<acre>(), NumFormat::Fp1);
            w.write_f64_field("crownArea", self.crown_fire_area.get::<acre>(), NumFormat::Fp1);
            w.write_f64_field("volume", self.volume.get::<acre_foot>(), NumFormat::Fp1);
            w.write_f64_field("population", self.population, NumFormat::Fp1);
            w.write_f64_field("realEstate", self.real_estate_value, NumFormat::Fp1);
            w.write_f64_field("land", self.land_value, NumFormat::Fp1);
            w.write_field("n_embers", self.n_embers);
            w.write_f64_field("pm25", self.pm25_release.get::<microgram>(), NumFormat::Fp1);
            w.write_f64_field("hrrPeak", self.hrr_peak.get::<megawatt>(), NumFormat::Fp1);
        })
    }
}

/// those are computed when we process the output rasters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputedStats {
    pub flin_max: LinearPowerDensity,
    pub fl_max: Length,
    pub vs_max: Velocity
}

impl JsonWritable for ComputedStats {
    fn write_json_to (&self, w: &mut JsonWriter) {
        w.write_object(|w| {
            w.write_f64_field("flinMax", self.flin_max.get::<watt_per_meter>() / 1000.0, NumFormat::Fp1); // we need kW/m
            w.write_f64_field("flMax", self.fl_max.get::<foot>(), NumFormat::Fp1);
            w.write_f64_field("vsMax", self.vs_max.get::<foot_per_minute>(), NumFormat::Fp1);
        })
    }
}

/// this captures the data we need to run a simulation on a single incident
#[derive(Debug, Clone, Serialize, Deserialize)]
//#[public_struct]
pub struct ElmFire {
    pub id: String, // this is supposed to be unique (but not user-facing)
    pub name: String, // this is what the user will see

    //--- location
    pub geo_center: GeoPoint, // center of fire simulation
    pub utm_rect: UtmRect, // geographic simulation bounds

    pub base_date: DateTime<Utc>, // this is the base date for the Elmfire run (full hour, likely before start_date)
    pub start_date: DateTime<Utc>, // of the simulation (usually first ignition)
    pub n_hours: u32, // duration of simulation in hours

    pub compute_wind: bool,  // do we use microgrid (computed) wind or just plain wx forecasts?

    //--- mandatory inputs
    pub ignitions: String,

    //--- optional inputs
    pub fixed: Option<String>, // if set use provided (fixed) wind- and fuel-moisture input
    pub spotting: Option<String>,
    pub barriers: Option<String>,

    pub dir: PathBuf, // where we keep inputs and outputs of this run
}

assert_impl_all!( ElmFire: Send, Sync );

impl ElmFire {

    pub fn new (
        id: String,
        name: String,
        geo_center: GeoPoint,
        bbox_size: Length,
        start_date: DateTime<Utc>,
        n_hours: u32,
        compute_wind: bool,
        ignitions: String,
        fixed: Option<String>,
        spotting: Option<String>,
        barriers: Option<String>

    )->Result<Self>
    {
        let utm_rect = get_utm_rect_around_point( &geo_center, bbox_size / 2.0)?;
        let base_date = full_hour( &start_date);
        let dir = CACHE_DIR.join( &id);

        Ok( ElmFire { id, name, n_hours, geo_center, utm_rect, base_date, start_date, compute_wind, ignitions, fixed, spotting, barriers, dir } )
    }

    pub fn from_point_ignition (
        name: String,
        date: DateTime<Utc>,
        geo_point: GeoPoint,
        bbox_size: Length,
        n_hours: u32,
        compute_wind: bool,
        fixed_wx: Option<String>,
        spotting: Option<String>,
        barriers: Option<String>
    ) -> Result<Self>
    {
        let id = Self::get_id( &name, &geo_point, &date);
        let base_date = full_hour(&date);
        let ignitions = point_ignition_src( &[(geo_point, (date - base_date).num_seconds())])?;

        Self::new( id, name, geo_point, bbox_size, date, n_hours, compute_wind, ignitions, fixed_wx, spotting, barriers)
    }

    // note - this should be kept in sync with odin_elmfire.js createRequest()
    pub fn get_id (name: &str, pos: &GeoPoint, date: &DateTime<Utc>)->String {
        format!("{}__{}__{}", name, date.format("%Y-%m-%d-%H%M%SZ"), pos.encode(4))
    }

    //.. and possibly more specialized constructors

    // number of forecast days to request from OpenMeteo so that we cover the full n_hours
    fn wx_forecast_days (&self)->u32 {
        get_fc_days( &self.base_date, hours(self.n_hours as u64))
    }

    fn get_wx_timestamps (&self)->Vec<i64> {
        let ts0 = self.base_date.timestamp();
        let n_steps = self.n_hours + 1;
        let mut tss: Vec<i64> = Vec::with_capacity(n_steps as usize);

        for i in 0..n_steps {
            tss.push( ts0 + (i * 3600) as i64)
        }

        tss
    }

    /// we use exterior mapping here so that no information is lost
    fn get_geo_rect (&self)->GeoRect {
        self.utm_rect.to_geo_rect( odin_common::utm::GeoRectMapping::ExteriorGeo)
    }

    fn get_output_dir (&self)->PathBuf {
        self.dir.join("outputs")
    }

    pub async fn create_inputs (&self, config: &ElmfireConfig)->Result<PathBuf> {
        self.prepare_run_dir()?;

        // TODO - this should support existing fire perimeters
        create_internal_input( &self, config)?;

        // this needs to be called before creating wx inputs since we need the DEM dataset if wind is computed
        create_landfire_input( &self, config).await?;

        if let Some(src) = &self.fixed { // run (mostly) on provided inputs
            let steps = parse_fixed_wx_lines( &src, self.n_hours as usize)?;
            create_fixed_input( &self, config, &steps)?;

        } else { // acquire live data from external sources
            create_wx_based_input( &self, config).await?; // this includes wind and computed (NFDRS) fuel moisture
        }

        self.create_datafile(config)
    }

    fn create_datafile (&self, config: &ElmfireConfig)->Result<PathBuf> {
        let input_dir = self.dir.join("inputs");
        let output_dir = self.get_output_dir();
        let scratch_dir = self.dir.join("scratch");

        let contents = ElmfireDataTemplate {
            input_dir: path_as_str( &input_dir)?,
            output_dir: path_as_str( &output_dir)?,
            scratch_dir: path_as_str( &scratch_dir)?,
            wx_dt_secs: config.input_dt.as_secs(),
            t_start_secs: self.t_start(),
            t_report_secs: self.t_report(),
            t_stop_secs: self.t_stop(),
            epsg: self.utm_rect.epsg(),
            lf_res_m: config.lf_res_m,
            x_min: self.utm_rect.x_min(),
            y_min: self.utm_rect.y_min(),
            n_steps: (self.n_hours + 1) as usize,
            ignitions: self.ignitions.as_str(),
            spotting: self.spotting.as_ref().map(|s| s.as_str())
            // barriers are specified in the INPUTS section (USE_BARRIERS=.TRUE. and BARRIER_FILENAME=...)
        }.to_string();

        let path = input_dir.join("elmfire.data");
        std::fs::write( &path, contents.as_str())?;
        Ok(path)
    }

    pub fn t_start (&self)->i64 {
        self.start_date.timestamp() - self.base_date.timestamp()
    }

    pub fn t_report (&self)->i64 {
        1800 // TODO - maybe we should make this configurable
    }

    pub fn t_stop (&self)->i64 {
        (self.n_hours * 3600) as i64 + 1800  // note we always compute 30min past n_hours to avoid contour artifacts in the outer ring
    }

    // seconds between first ignition timestamp and the base wx timestamp
    pub fn secs_from_base (&self, d: DateTime<Utc>)->i64 {
        d.timestamp() - self.base_date.timestamp()
    }

    pub fn prepare_run_dir (&self)->Result<()> {
        let input_dir = self.dir.join("inputs");
        let output_dir = self.dir.join("outputs");
        let scratch_dir = self.dir.join("scratch");

        if input_dir.is_dir() { remove_dir_all( &input_dir); }
        if output_dir.is_dir() { remove_dir_all( &output_dir); }
        if scratch_dir.is_dir() { remove_dir_all( &scratch_dir); }

        create_dir_all( &input_dir)?;
        create_dir_all( &output_dir)?;
        create_dir_all( &scratch_dir)?;

        Ok(())
    }

    pub fn process_outputs (&self, config: &ElmfireConfig)->Result<ElmfireOutput> {
        let mut area: Vec<AreaSnapshot> = Vec::with_capacity(0);
        let mut stats: Vec<FireStats> = Vec::with_capacity(0);
        let mut files: Vec<String> = Vec::new();

        let output_dir = self.get_output_dir();

        // fireline intensity output raster (always on so we use it to get the geo_rect)
        let ds = process_raster_output(&output_dir, &FLIN_RE, "flin_wgs84.tif", "flin.png", Some(0.02..0.99))?;
        let geo_rect = odin_gdal::get_geo_rect(&ds)?; // will be the same for all so we just need to compute once
        let (flin_min,flin_max) = as_min_max( odin_gdal::get_band_value_range( &ds, 1)? );
        files.push( "flin.png".into());

        // spread rate output raster
        let ds = process_raster_output(&output_dir, &VS_RE, "vs_wgs84.tif", "vs.png", Some(0.02..0.99))?;
        let (vs_min, vs_max) = as_min_max( odin_gdal::get_band_value_range( &ds, 1)? );
        files.push("vs.png".into());

        // flame length raster
        let ds = process_raster_output(&output_dir, &FL_RE, "fl_wgs84.tif", "fl.png", Some(0.02..0.99))?;
        let (fl_min, fl_max) = as_min_max( odin_gdal::get_band_value_range( &ds, 1)? );
        files.push("fl.png".into());

        // crown fire raster (don't adjust ranges)
        process_raster_output(&output_dir, &FL_RE, "cf_wgs84.tif", "cf.png", None)?;
        files.push("cf.png".into());

        if let Some(area_path) = find_first_match_in_dir( &output_dir, &AREA_RE) {
            area = parse_area( &area_path, self.base_date)?;
        }

        if let Some(stats_path) = find_first_match_in_dir( &output_dir, &STATS_RE) {
            stats = parse_fire_stats( &stats_path, self.base_date)?;
        }

        let comp_stats = ComputedStats {
            flin_max: LinearPowerDensity::new::<watt_per_meter>(flin_max * 1000.0), // we get it in kW/m
            fl_max: Length::new::<foot>(fl_max),
            vs_max: Velocity::new::<foot_per_minute>(vs_max)
        };

        // time-of-arrival contours (we might have to pre-process toa_wgs84.tif if this is a small fire)
        if let Some(utm_path) = find_first_match_in_dir( &output_dir, &TOA_RE) {
            let wgs84_path = output_dir.join("toa_wgs84.tif");
            let mut ds = odin_gdal::warp::warp_to_wgs84( &utm_path, &wgs84_path, vec![-9999.0])?;

            if let Some(final_area) = area.last().map(|a| a.area) {
                if self.is_small_area_fire( config, final_area) {
                    let mod_wgs84_path = output_dir.join("toa_wgs84_mod.tif");
                    ds = pre_process_small_toa_raster( &ds, self.n_hours, &mod_wgs84_path)?;
                }
            }

            let json_path = output_dir.join("toa.geojson");
            let levels = contour_levels( self.n_hours);
            odin_gdal::contour::to_fixed_level_geo_json( &ds, 1, &levels, &json_path)?;

            files.push("toa.geojson".into());
        }

        Ok( ElmfireOutput { stats, area, comp_stats, files, geo_rect } )
    }

    fn is_small_area_fire (&self, config: &ElmfireConfig, final_area: Area)->bool {
        let cell_area = (config.lf_res_m * config.lf_res_m) as f64;
        (final_area.get::<square_meter>() / cell_area) as usize <= config.small_fire_cells
    }

    async fn execute (&self, config: &ElmfireConfig)->Result<ElmfireOutput> {
        let path = self.create_inputs( config).await?;
        let output = run_captured_cmd!( &config.elmfire_cmd, path_as_str( &path)?).await?;
        if output.status.success() {
            self.process_outputs( config)

        } else {
            let xc = output.status.code().unwrap_or(-1);
            eprintln!("Elmfire execution failed: {}", xc);
            eprintln!( "{}", String::from_utf8_lossy( &output.stderr));

            Err( op_failed!("Elmfire failed to execute with code {}", xc) )
        }
    }
}

/// generate (UTM-based) Elmfire ignitions source for given ignition-points
/// note that all points need to have the same utm_zone
pub fn point_ignition_src (igns: &[(GeoPoint,i64)])->Result<String> {
    let mut s = format!("NUM_IGNITIONS={}\n", igns.len());
    for (i, (geo_p,tsec)) in igns.iter().enumerate() {
        let utm_p: UtmPoint = geo_to_utm( geo_p).ok_or( op_failed!("invalid point ignition coordinates"))?;
        let idx = i+1;
        s.push_str( &format!("T_IGN({}) = {}\n", idx, tsec));
        s.push_str( &format!("X_IGN({}) = {:.0}\n", idx, utm_p.easting));
        s.push_str( &format!("Y_IGN({}) = {:.0}\n", idx, utm_p.northing));
    }

    Ok( s )
}

/// the time steps (in seconds) for which we want to compute time-of-arival contour isochlines
/// this should always be one half-hour isocline less than the requested n_hours so that we avoid jagged contour artifacts
pub fn contour_levels (n_hours: u32)->Vec<f64> {
    let max = (n_hours * 2) as usize;
    let mut levels: Vec<f64> = Vec::with_capacity(max);
    for i in 1..=max {
        levels.push( i as f64 * 1800.0)
    }
    levels
}


/// to avoid artifacts and jagged outer contour lines for small fires we have to crop the image to the data range
/// (plus some nodata margin), then replace the nodata values with a max value beyond our upper contour limit
/// and finally upsample using smooth interpolation
pub fn pre_process_small_toa_raster<P: AsRef<Path>> (ds: &Dataset, n_hours: u32, path: P)->Result<Dataset> {
    let (n_cols, n_rows) = ds.raster_size();
    let margin: usize = 10;

    let idx_bounds = get_exterior_data_bounds( ds, 1)?.apply_margin( margin, n_cols, n_rows);

    let mut cropped_ds = crop_mem_dataset( &ds, &idx_bounds)?;

    // replace nodata value with some value that exceeds our outermost isochrone so that we can smoothly
    // interpolate it (otherwise interpolation will preserve the outer cell bounds despite upsampling)
    let max_toa = ((n_hours + 1) * 3600) as f32;
    replace_rasterband_value( &mut cropped_ds, 1, -9999.0, max_toa)?;

    // now up-sample by a factor of 2 to smoothen countour lines
    let ri = get_raster_info( &cropped_ds)?;
    let x_res = ri.dx / 2.0;
    let y_res = ri.dy / 2.0;

    let mut warper = SimpleWarpBuilder::new( &cropped_ds, path)?;
    warper.set_tgt_resolution( x_res, y_res);
    warper.set_resample_alg( ResampleAlg::CubicSpline);
    warper.set_tgt_format("GTiff");
    let tgt_ds = warper.exec()?;

    Ok( tgt_ds )
}

fn process_raster_output (output_dir: &PathBuf, utm_re: &Regex, wgs84_fname: &str, png_fname: &str, pct_range: Option<Range<f64>>)->Result<Dataset>
{
    if let Some(utm_path) = find_first_match_in_dir( &output_dir, utm_re) {
        let wgs84_path = output_dir.join(wgs84_fname);
        let ds = odin_gdal::warp::warp_to_wgs84( &utm_path, &wgs84_path, vec![-9999.0])?;

        let img_path = output_dir.join(png_fname);

        let cmap = vec![0xff000000, 0xff000040, 0xff000080, 0xa00000ff, 0xff0000ff, 0xff4500ff, 0xffa500ff, 0xffd700ff, 0xffff00ff];
        odin_gdal::colorize::to_cmap( &img_path, &ds, 1, pct_range, &cmap)?;
        //odin_gdal::colorize::to_alpha( &img_path, &ds, 1, 0xffffff, pct_range)?; // target color should be configured

        Ok(ds)

    } else {
        Err( op_failed!("no match for {:?}", utm_re) )
    }
}

#[derive(Template)]
#[template( path = "elmfire.askama")]
struct ElmfireDataTemplate<'a> {
    input_dir: &'a str,
    output_dir: &'a str,
    scratch_dir: &'a str,
    wx_dt_secs: u64,
    t_start_secs: i64,
    t_report_secs: i64,
    t_stop_secs: i64,
    epsg: u32,
    lf_res_m: u32,
    x_min: f64,
    y_min: f64,
    n_steps: usize,
    ignitions: &'a str,
    spotting: Option<&'a str>,
    // note that barriers are specified in the INPUTS section
}


pub fn get_utm_rect_around_point (center: &GeoPoint, dist: Length)->Result<UtmRect> {
    let mut p = geo_to_utm( center).ok_or( op_failed!("center not valid UTM"))?;
    p.round();
    let dm = dist.get::<meter>();

    let mut utm_rect = UtmRect::from_wsen_meters(p.easting - dm, p.northing - dm, p.easting + dm, p.northing + dm, p.utm_zone);
    utm_rect.round();

    Ok( utm_rect )
}

pub fn parse_area<P: AsRef<Path>> (path: P, base_date: DateTime<Utc>)->Result<Vec<AreaSnapshot>> {
    let mut vs: Vec<AreaSnapshot> = Vec::new();

    let mut rdr = csv::Reader::from_reader( File::open(&path)?);
    for result in rdr.records() {
        let record = result?;
        if record.len() != 2 { return Err( op_failed!("not an Acres Record") ) }

        let ts: f64 = record[0].trim().parse()?;
        let acres: f64 = record[1].trim().parse()?;

        let date = base_date + Duration::from_secs_f64(ts);
        let area = Area::new::<acre>( acres);

        vs.push( AreaSnapshot { date, area } )
    }

    Ok( vs )
}

/// format:
/// icase,Meteorology band, x, y, tstop (h), Wall clock time (s), Total fire area (ac), Crown fire area (ac),
/// Fire volume (ac-ft), Population affected, Real estate value, Land value, Nembers, Pyrome, Containfrac,
/// PM 2.5 release (ug), HRR peak (MW), Start timestamp, Fire id, Astop (ac)
pub fn parse_fire_stats<P: AsRef<Path>> (path: P, base_date: DateTime<Utc>)->Result<Vec<FireStats>> {
    let mut vs: Vec<FireStats> = Vec::new();

    let mut rdr = csv::Reader::from_reader( File::open(&path)?);
    for result in rdr.records() {
        let record = result?;
        if record.len() != 20 { return Err( op_failed!("not an FireSizeStats Record") ) }

        let h: f64 = record[4].trim().parse()?;
        let date = base_date + Duration::from_hours(h as u64);

        let ac: f64 = record[6].trim().parse()?;
        let total_area = Area::new::<acre>(ac);

        let ac: f64 = record[7].trim().parse()?;
        let crown_fire_area = Area::new::<acre>(ac);

        let ac_ft: f64 = record[8].trim().parse()?;
        let volume = Volume::new::<acre_foot>(ac_ft);

        let population: f64 = record[9].trim().parse()?;
        let real_estate_value: f64 = record[10].trim().parse()?;
        let land_value: f64 = record[11].trim().parse()?;

        let n_embers: u32 = record[12].trim().parse()?;

        let ug: f64 = record[15].trim().parse()?;
        let pm25_release = Mass::new::<microgram>(ug);

        let mw: f64 = record[16].trim().parse()?;
        let hrr_peak = Power::new::<megawatt>( mw);

        vs.push( FireStats{date,total_area,crown_fire_area,volume,population,real_estate_value,land_value,n_embers,pm25_release,hrr_peak})
    }

    Ok( vs )
}

/// aggregate to bundle simulation spec and results.
/// we need to wrap the ElmFire instance into an Arc so that it can be easily cloned
/// Note that results are set delayed, i.e. we need to wrap into an Option
/// Note also that we cannot use OdinElmfireResult to wrap the result since it is not Send (because of wrapped 3rd party errors)
#[derive(Debug,Clone, Serialize,Deserialize)]
pub struct ElmfireRun {
    pub fire: Arc<ElmFire>, // needs to be Arc so that we can easily clone
    pub result: Option<StdResult<ElmfireOutput,String>> // not set before the run is done but should also preserve failure
}

assert_impl_all!( ElmfireRun: Send, Sync);

impl ElmfireRun {

    pub fn get_json_update_msg (&self)->String {
        let mut w = JsonWriter::with_capacity(8192);
        self.write_json_to( &mut w);
        let msg = w.as_str();
        ws_msg_from_json( ElmfireService::mod_path(), "update", &msg)
    }

    pub fn save_to<P: AsRef<Path>> (&self, path: P)->Result<()> {
        let s = ron::ser::to_string(self)?;
        let mut file = File::create(path)?;
        file.write_all( s.as_bytes())?;
        Ok(())
    }

    pub fn read_from<P: AsRef<Path>> (path: P)->Result<Self> {
        let s = filepath_contents_as_string(path)?;
        let elmfire_run: Self = ron::de::from_str( &s)?;
        Ok(elmfire_run)
    }
}

/// this is the serialization for what is sent to clients (processed by odin_elmfire.js)
impl JsonWritable for ElmfireRun {
    fn write_json_to (&self, w: &mut JsonWriter) {
        let fire = self.fire.as_ref();
        let center = &fire.geo_center;
        let geo_rect = fire.get_geo_rect();

        w.write_object( |w| {
            w.write_field( "id", &fire.id);
            w.write_field( "name", &fire.name);
            w.write_pos_field( "pos", center, NumFormat::Fp4);
            w.write_date_field( "date", fire.start_date);
            w.write_rect_field( "bbox", &geo_rect, NumFormat::Fp4);
            w.write_field( "nHours", fire.n_hours);
            w.write_field( "computeWind", fire.compute_wind);

            // NOTE - no '\n' in JSON strings allowed, we have to escape/un-escape manually
            w.write_field( "ignitions", JsonEscapedStr(&fire.ignitions));

            if let Some(fixed) = fire.fixed.as_ref() {
                w.write_field( "fixed", JsonEscapedStr(fixed))
            }
            if let Some(spotting) = fire.spotting.as_ref() {
                w.write_field( "spotting", JsonEscapedStr(spotting))
            }
            if let Some(barriers) = fire.barriers.as_ref() {
                w.write_field( "barriers", JsonEscapedStr(barriers))
            }

            if let Some(result) = &self.result {
                match result {
                    Ok(outputs) => {
                        w.write_object_field( "output", |w|{
                            w.write_array_field("stats", |w|{
                                for s in outputs.stats.iter() {
                                    s.write_json_to(w);
                                }
                            });
                            w.write_array_field("area", |w|{
                                for a in outputs.area.iter() {
                                    a.write_json_to(w);
                                }
                            });
                            w.write_json_field("compStats", &outputs.comp_stats);
                            w.write_array_field( "files", |w|{
                                for fname in outputs.files.iter() {
                                    w.write_value( fname);
                                }
                            });

                            // this is the bbox for the generated WGS84 imagery
                            w.write_rect_field("bbox", &outputs.geo_rect, NumFormat::Fp5);
                        });
                    }
                    Err(e) => {
                        w.write_field("error", e);
                    }
                }
            }
        });
    }
}

/* #region stored namelist support **********************************************************************/

// TODO - this should be not tied to NML

pub fn get_saved_list<P: AsRef<Path>> (dir: P, fname_re: &Regex)->Result<Vec<String>> {
    let mut list: Vec<String> = Vec::new();

    for pn in matching_files_in_dir( &dir, fname_re)? {
        if let Some(fname) = filestem( &pn) {
            list.push( fname.into()) // we only store the filename stem (no extension)
        }
    }
    list.sort();

    Ok( list )
}

pub fn get_contents<P: AsRef<Path>> (dir: P, name: &str)->Option<String> {
    let fname = format!("{}.nml", name);
    let file_path = dir.as_ref().join(fname);

    filepath_contents_as_string(file_path).ok()
}

pub fn save_contents<P: AsRef<Path>> (dir: P, name: &str, new_content: &str)->Result<()> {
    // TODO - this should sanitize the source to see if it is valid Elmfire nml
    // note that Elmfire uses the slice operator ':' (hence nml crate does not work)
    let fname = format!("{}.nml", name);
    Ok( set_filepath_contents( dir, &fname, new_content.as_bytes())? )
}

pub fn remove_contents<P: AsRef<Path>> (dir: P, name: &str)->Result<()> {
    // TODO - this should sanitize the source to see if it is valid Elmfire nml
    // note that Elmfire uses the slice operator ':' (hence nml crate does not work)
    let fname = format!("{}.nml", name);
    let path = dir.as_ref().join( fname);
    Ok( remove_file(&path)? )
}

/* #endregion stored namelist support */
