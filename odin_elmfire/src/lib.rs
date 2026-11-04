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
use interp::{InterpMode, interp_slice};
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
    BoundingBox, IndexBounds2D, angle::Angle360, apply_index_margin, as_min_max,
    datetime::{self, as_minutes, full_hour, hours, minutes, secs_f64, short_utc_datetime_string, to_epoch_millis, ser_epoch_millis, de_from_epoch_millis},
    fs::{ EnvPathBuf, ensure_writable_dir, filepath_contents_as_string, filestem, find_first_match_in_dir, matching_files_in_dir, path_as_str, set_filepath_contents},
    geo::{GeoPoint,GeoRect, GeoLine, GeoLineString, GeoEllipse},
    json_writer::{JsonEscapedStr, JsonWritable, JsonWriter, NumFormat}, run_captured_cmd,
    utm::{UtmPoint, UtmRect, geo_to_utm, try_geo_to_utm_zone},
};
use odin_gdal::{
    colorize::to_cmap,
    copy_mem_dataset, crop_dataset, crop_mem_dataset,
    geods::GeoDs,
    get_band_value_range, get_exterior_data_bounds, get_geo_rect, get_gtiff_create_opts, get_gtiff_driver, get_mem_driver,
    get_raster_info, get_rasterband_as_vec, get_resolution, open_dataset, read_rasterband_into_slice, replace_rasterband_value,
    warp::{ResampleAlg, SimpleWarpBuilder, warp_to_res_mem, warp_to_res_tif, warp_to_wgs84_mem, warp_to_wgs84_tif}
};
use odin_openmeteo::get_fc_days;
use odin_landfire::{evt_cats::{EVT_CATEGORIES, EvtCategories}, get_evt_values_at_utm_points};
use odin_server::{spa::SpaService,ws_service::ws_msg_from_json};

use static_assertions::assert_impl_all;

pub mod wind;
use wind::{create_wx_windfields,create_computed_windfields};

pub mod input;
use input::{create_fixed_input, create_internal_input, create_landfire_input, create_wx_based_input, resample_computed_inputs};

pub mod barrier;
use barrier::{DropAgent, apply_air_drops, create_fuel_break_input, default_drop_agents, default_fuel_break_fbfm40, uses_barrier_raster};

pub mod actor;

pub mod service;
pub use service::ElmfireService;

pub mod fixed;
pub use fixed::FixedInput;

pub mod toa;
pub use toa::{contour_levels, process_toa_contours};

pub mod errors;
use errors::{OdinElmfireError,Result,op_failed};


lazy_static! {
    pub static ref CACHE_DIR: PathBuf = { pkg_cache_dir!() };
    pub static ref DATA_DIR: PathBuf = { pkg_data_dir!() };

    pub static ref SPOTTING_DIR: PathBuf = DATA_DIR.join("spotting");
    pub static ref FIXED_DIR: PathBuf = DATA_DIR.join("fixed");
    pub static ref NML_RE: Regex = Regex::new( r#"^[^._].*\.nml"#).unwrap(); // namelist file pattern

    pub static ref FLIN_RE: Regex = Regex::new( r#"flin_\d+_\d+.*\.tif"#).unwrap();
    pub static ref TOA_RE: Regex = Regex::new( r#"time_of_arrival_.*\.tif"#).unwrap();
    pub static ref VS_RE: Regex = Regex::new( r#"vs_\d+_\d+.*\.tif"#).unwrap();
    pub static ref CF_RE: Regex = Regex::new( r#"crown_fire_.*\.tif"#).unwrap();
    pub static ref FL_RE: Regex = Regex::new( r#"flame_length_.*\.tif"#).unwrap();
    pub static ref AREA_RE: Regex = Regex::new( r#"acres_.*\.csv"#).unwrap();
    pub static ref STATS_RE: Regex = Regex::new( r#"fire_size_stats.*\.csv"#).unwrap();

    pub static ref EF_RUN_RE: Regex = Regex::new( r#"elmfire.ron"#).unwrap();
}

const WX_DT: Duration = Duration::from_secs( 60 * 15);

define_load_config!{}
define_load_asset!{}

fn default_uniform_res()->bool { true }

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

    // resample all computed (fuel moisture and wind) inputs to the landfire grid (lf_res_m). This is required for
    // Elmfire builds that do not handle different input raster resolutions (they use the wind rasters as reference)
    #[serde(default="default_uniform_res")]
    uniform_res: bool,

    // fuel breaks and air drops (see barrier.rs)
    #[serde(default="default_fuel_break_fbfm40")]
    fuel_break_fbfm40: i16,       // fuel model for fuel breaks that are at least lf_res_m wide (default NB9)
    #[serde(default="default_drop_agents")]
    drop_agents: Vec<DropAgent>,  // air drop agents (e.g. retardant, water) and their fuel moisture effects

    input_dt: Duration,       // meteorologicel time step length
    small_fire_cells: usize,  // fires with less burned cells get up-sampled before contouring (to avoid pixelized contour artifacts)

    elmfire_cmd: EnvPathBuf,   // the Elmfire executable

    // Elmfire raster output colorization
    toa_colors: Vec<u32>,
    flin_colors: Vec<u32>,
    vs_colors: Vec<u32>,
    cf_colors: Vec<u32>,
    fl_colors: Vec<u32>,
}


/// this represents the translated outcome of a Elmfire run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElmfireOutput {
    pub stats: Vec<FireStats>, // at the end of the simulation
    pub timesteps: Vec<Timestep>,
    pub windfields: Vec<WindField>,
    pub comp_stats: ComputedStats,
    pub raster_products: Vec<(String,GeoRect)>
}

assert_impl_all!( ElmfireOutput : Send,Sync);

/// those we read from the Elmfire generated acres*.csv file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timestep {
    pub date: DateTime<Utc>,
    pub area: Area,
    pub growth: Ratio
}

impl JsonWritable for Timestep {
    fn write_json_to (&self, w: &mut JsonWriter) {
        w.write_object(|w| {
            w.write_date_field("date", self.date);
            w.write_f64_field("area", self.area.get::<acre>(), NumFormat::Fp1);
            w.write_f64_field("growth", self.growth.get::<percent>(), NumFormat::Fp0);
        });
    }
}

// timestep associated set of wind field filenames for grid- and vector- CSV filenames and contour GeoJSON filename
// this is computed from the wind input we use to run Elmfire
// Note that the Elmfire wind input is for full hours so we have to interpolate to respective Timestep dates.
// There should be a 1:1 relation between Timestep and WindField vecs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindField {
    pub date: DateTime<Utc>, // this is the same as the corresponding Timestep
    pub grid: String, // file name for grid CSV (for particle animation)
    pub vectors: String, // file name for vector CSV (vector display)
    pub contours: String // file name for windspeed contour GeoJSON
}

impl JsonWritable for WindField {
    fn write_json_to (&self, w: &mut JsonWriter) {
        w.write_object(|w| {
            w.write_date_field("date", self.date);
            w.write_field("grid", &self.grid);
            w.write_field("vectors", &self.vectors);
            w.write_field("contours", &self.contours);
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

/// note that this has to be kept compatible with the odin_elmfire.js BarrierEntry class
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Barrier {
    name: String,
    barrier_type: String,
    #[serde(with="length_ft")]
    width: Length, // (de)serialized in feet (as used by the UI)
    value: BarrierValue,

    // the following fields are only used for air drops (see barrier.rs)
    #[serde(default)]
    step: u32, // hourly input time step from which the drop is in effect (0 is the full hour before first ignition)
    #[serde(default)]
    agent: Option<String>, // name of the drop agent (ElmfireConfig.drop_agents) - None for fuel breaks
}

impl Barrier {
    /// air drops have a drop agent, all other barriers are fuel breaks
    pub fn is_air_drop (&self)->bool {
        self.agent.as_ref().is_some_and( |a| !a.is_empty())
    }

    pub fn value (&self)->&BarrierValue { &self.value }

    pub fn geo_points (&self)->Vec<GeoPoint> {
        match &self.value {
            BarrierValue::GeoLine(line) => vec![ line.start(), line.end() ],
            BarrierValue::GeoLineString(line_string) => line_string.as_geo_points(),
            BarrierValue::GeoEllipse(ellipse) => ellipse.outline_points( 32)
        }
    }
}

/// untagged since odin_elmfire.js sends plain (odin_server/main.js) GeoLineString `{points}` and GeoLine `{start,end}` objects
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BarrierValue {
    GeoLineString(GeoLineString),
    GeoLine(GeoLine),
    GeoEllipse(GeoEllipse), // bucket drop patterns
}

/// serde support for lengths that are (de)serialized as feet
mod length_ft {
    use serde::{Deserialize, Deserializer, Serializer};
    use uom::si::{f64::Length, length::foot};

    /// rounded to 1ft (avoids conversion artifacts such as 9.999999999999998)
    pub fn serialize<S: Serializer> (len: &Length, s: S)->Result<S::Ok, S::Error> {
        s.serialize_f64( len.get::<foot>().round())
    }

    pub fn deserialize<'de, D: Deserializer<'de>> (d: D)->Result<Length, D::Error> {
        Ok( Length::new::<foot>( f64::deserialize(d)?))
    }
}

impl JsonWritable for BarrierValue {
    fn write_json_to (&self, w: &mut JsonWriter) {
        match self {
            BarrierValue::GeoLine(value) => value.write_json_to( w),
            BarrierValue::GeoLineString(value) => value.write_json_to( w),
            BarrierValue::GeoEllipse(value) => value.write_json_to( w)
        }
    }
}

impl JsonWritable for Barrier {
    fn write_json_to (&self, w: &mut JsonWriter) {
        w.write_object(|w| {
            w.write_field( "name", &self.name);
            w.write_field( "barrierType", &self.barrier_type);
            w.write_f64_field( "width", self.width.get::<foot>(), NumFormat::Fp0);
            w.write_json_field( "value", &self.value);
            w.write_field( "step", self.step);
            if let Some(agent) = &self.agent { w.write_field( "agent", agent); }
        });
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Ignition {
    id: usize,
    ignition_type: String,
    #[serde(serialize_with="ser_epoch_millis", deserialize_with="de_from_epoch_millis")]
    date: DateTime<Utc>, // epoch millis (as sent by odin_elmfire.js and written by JsonWritable)
    value: IgnitionValue
}

impl Ignition {
    pub fn new_point (id: usize, date: DateTime<Utc>, p: GeoPoint)->Self {
        Ignition { id, ignition_type: "point".to_string(), date, value: IgnitionValue::Point(p) }
    }

    pub fn new_line_string (id: usize, date: DateTime<Utc>, ls: GeoLineString)->Self {
        Ignition { id, ignition_type: "line".to_string(), date, value: IgnitionValue::LineString(ls) }
    }
}

/// create the NML source for a list of ignitions. Note that the elmfire.data NML format according
/// to https://elmfire.io/ELMFIRE_Guide.pdf only supports point and line ignitions. The format is
///
/// ```text
/// NUM_IGNITIONS = <number of point ignitions>
/// T_IGN(1) = <full secs between first point ignition date and base_date>
/// X_IGN(1) = <UTM easting of first ignition point>
/// Y_IGN(1) = <UTM northing of first ignition point>
/// ...
/// T_LINE_IGN(1) = <full secs between first line ignition date and base_date>
/// X_LINE_IGN_START(1) = <UTM easting of first line ignition start point>
/// Y_LINE_IGN_START(1) = <UTM northing of first line ignition start point>
/// X_LINE_IGN_END(1) = <UTM easting of first line ignition end point>
/// Y_LINE_IGN_END(1) = <UTM northing of first line ignition end point>
/// ...
/// ```
/// Since elmfire does not (yet?) support lineStrings we have to break up GeoLineString values
/// into separate lines that all have the same T_LINE_IGN value.
/// All ignition points are converted into the UTM zone of `geo_center` (the center of the simulation grid), even if
/// they are located in a neighboring zone. Ignition points that cannot be converted cause an error
fn ignitions_to_nml (base_date: DateTime<Utc>, geo_center: &GeoPoint, ignitions: &[Ignition])->Result<String> {
    let utm_zone = geo_to_utm( geo_center).ok_or( op_failed!("invalid simulation center {geo_center}"))?.utm_zone;
    let utm = |p: &GeoPoint| try_geo_to_utm_zone( p, utm_zone);

    let mut points: Vec<(i64,UtmPoint)> = Vec::new();               // (t_ign, point)
    let mut lines: Vec<(i64,UtmPoint,UtmPoint)> = Vec::new();       // (t_line_ign, start, end) - one per GeoLineString leg

    for ign in ignitions {
        let t = (ign.date - base_date).num_seconds();
        match &ign.value {
            IgnitionValue::Point(p) => {
                points.push( (t, utm(p)?));
            }
            IgnitionValue::LineString(ls) => {
                let utm_ps: Vec<UtmPoint> = ls.as_geo_points().iter().map( |p| utm(p)).collect::<std::result::Result<_,_>>()?;
                for leg in utm_ps.windows(2) {
                    lines.push( (t, leg[0].clone(), leg[1].clone()));
                }
            }
        }
    }

    let mut s = String::with_capacity( 64 + points.len()*64 + lines.len()*128);

    s.push_str( &format!("NUM_IGNITIONS = {}\n", points.len())); // only point ignitions
    for (i, (t, p)) in points.iter().enumerate() {
        let idx = i+1;
        s.push_str( &format!("T_IGN({}) = {}\n", idx, t));
        s.push_str( &format!("X_IGN({}) = {:.0}\n", idx, p.easting));
        s.push_str( &format!("Y_IGN({}) = {:.0}\n", idx, p.northing));
    }

    for (i, (t, p0, p1)) in lines.iter().enumerate() {
        let idx = i+1;
        s.push_str( &format!("T_LINE_IGN({}) = {}\n", idx, t));
        s.push_str( &format!("X_LINE_IGN_START({}) = {:.0}\n", idx, p0.easting));
        s.push_str( &format!("Y_LINE_IGN_START({}) = {:.0}\n", idx, p0.northing));
        s.push_str( &format!("X_LINE_IGN_END({}) = {:.0}\n", idx, p1.easting));
        s.push_str( &format!("Y_LINE_IGN_END({}) = {:.0}\n", idx, p1.northing));
    }

    Ok(s)
}

impl JsonWritable for Ignition {
    fn write_json_to (&self, w: &mut JsonWriter) {
        w.write_object(|w| {
            w.write_field( "id", self.id);
            w.write_date_field( "date", self.date);
            w.write_field("ignitionType", &self.ignition_type);
            w.write_json_field( "value", &self.value);
        });
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IgnitionValue {
    Point(GeoPoint),
    LineString(GeoLineString)
}

impl JsonWritable for IgnitionValue {
    fn write_json_to (&self, w: &mut JsonWriter) {
        match self {
            IgnitionValue::Point(value) => value.write_json_to( w),
            IgnitionValue::LineString(value) => value.write_json_to( w),
        }
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

    pub base_date: DateTime<Utc>, // this is the base date for the Elmfire run (full hour time before start date). Basis for wx-retrieval
    pub start_date: DateTime<Utc>, // of the simulation (usually first ignition)
    pub n_hours: u32, // duration of simulation in hours

    pub compute_wind: bool,  // do we use microgrid (computed) wind or just plain wx forecasts?

    //--- mandatory inputs
    pub ignitions: Vec<Ignition>, // converted into elmfire.data NML by ignitions_to_nml()

    //--- optional inputs
    pub fixed: Option<FixedInput>, // if set use provided (fixed) wind- and fuel-moisture input
    pub spotting: Option<String>,
    pub barriers: Vec<Barrier>,

    pub dir: PathBuf, // where we keep inputs and outputs of this run
    pub overwrite: bool
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
        ignitions: Vec<Ignition>,
        fixed: Option<FixedInput>,
        spotting: Option<String>,
        barriers: Vec<Barrier>,
        overwrite: bool

    )->Result<Self>
    {
        let utm_rect = get_utm_rect_around_point( &geo_center, bbox_size / 2.0)?;
        let base_date = full_hour( &start_date);
        let dir = CACHE_DIR.join( &id);

        Ok( ElmFire {
            id, name, n_hours, geo_center, utm_rect, base_date, start_date, compute_wind, ignitions, fixed, spotting, barriers, dir, overwrite
        } )
    }

    pub fn from_point_ignition (
        name: String,
        date: DateTime<Utc>,
        geo_point: GeoPoint,
        bbox_size: Length,
        n_hours: u32,
        compute_wind: bool,
        fixed_wx: Option<FixedInput>,
        spotting: Option<String>,
        barriers: Vec<Barrier>
    ) -> Result<Self>
    {
        let id = Self::get_id( &name, &geo_point, &date);
        let ignitions = vec![ Ignition::new_point( 1, date, geo_point) ];

        Self::new( id, name, geo_point, bbox_size, date, n_hours, compute_wind, ignitions, fixed_wx, spotting, barriers, false)
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

        // fuel breaks modify fbfm40.tif and create barriers.tif (which uses fbfm40.tif as reference grid)
        create_fuel_break_input( &self, config)?;

        if let Some(fixed) = &self.fixed { // run (mostly) on provided inputs
            create_fixed_input( &self, config, fixed).await?;

        } else { // acquire live data from external sources
            create_wx_based_input( &self, config).await?; // this includes wind and computed (NFDRS) fuel moisture
        }

        // fuel moisture and wind inputs are computed with their own resolution (wx_res_m, wind_res_m) but current Elmfire
        // builds require all inputs to have the same grid, i.e. we resample them to the landfire grid (if configured)
        resample_computed_inputs( &self, config)?;

        // air drops modify the (resampled) fuel moisture inputs
        apply_air_drops( &self, config)?;

        self.create_datafile(config)
    }

    fn create_datafile (&self, config: &ElmfireConfig)->Result<PathBuf> {
        let input_dir = self.dir.join("inputs");
        let output_dir = self.get_output_dir();
        let scratch_dir = self.dir.join("scratch");
        let ignitions = ignitions_to_nml( self.base_date, &self.geo_center, &self.ignitions)?;

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
            ignitions: ignitions.as_str(),
            spotting: self.spotting.as_ref().map(|s| s.as_str()),
            use_barriers: uses_barrier_raster( &self, config)
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

    /// this is simulation end time in seconds relative to base date so we have to take the first ignition into account
    pub fn t_stop (&self)->i64 {
        self.t_start() + (self.n_hours * 3600) as i64
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
        let dir = self.get_output_dir();

        let raster_products = process_ef_raster_output(config, &dir)?;

        let ds = open_dataset( &dir.join("toa_wgs84.tif"))?;
        process_toa_contours( &ds, &dir.join("toa.geojson"), &dir.join("toa_poly.geojson"), config.small_fire_cells, self.t_start(), self.n_hours)?;

        let flin_range = get_band_value_range( &open_dataset( &dir.join("flin_wgs84.tif"))?, 1)?;
        let vs_range = get_band_value_range( &open_dataset( &dir.join("vs_wgs84.tif"))?, 1)?;
        let fl_range = get_band_value_range( &open_dataset( &dir.join("fl_wgs84.tif"))?, 1)?;

        let cell_size = Area::new::<square_meter>( (config.lf_res_m as f64).powi(2));
        let timesteps = compute_timesteps_from_toa( &dir.join("toa.tif"), &self.start_date, &contour_levels(self.t_start(), self.n_hours), cell_size)?;
        let windfields = if self.compute_wind {
            create_computed_windfields( &self.dir, &self.base_date, &timesteps, config.wind_res_m)?
        } else {
            create_wx_windfields( &self.dir, &self.base_date, &timesteps, config.wind_res_m)?
        };

        let path = find_first_match_in_dir( &dir, &STATS_RE).ok_or( op_failed!("no fire_size_stats"))?;
        let stats = parse_fire_stats( &path, self.base_date)?;

        let comp_stats = ComputedStats {
            flin_max: LinearPowerDensity::new::<watt_per_meter>(flin_range.end * 1000.0), // we get it in kW/m
            fl_max: Length::new::<foot>(fl_range.end),
            vs_max: Velocity::new::<foot_per_minute>(vs_range.end)
        };

        Ok( ElmfireOutput { stats, timesteps, windfields, comp_stats, raster_products } )
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

fn get_ds (output_dir: &PathBuf, regex: &Regex)->Result<Dataset> {
    let path = find_first_match_in_dir( output_dir, regex).ok_or( op_failed!("no '{}' output", regex.to_string()))?;
    Ok( open_dataset( &path)? )
}

pub fn process_ef_raster_output (config: &ElmfireConfig, dir: &PathBuf)->Result<Vec<(String,GeoRect)>> {
    let mut raster_products = Vec::with_capacity(5);

    let toa_ds = get_ds( &dir, &TOA_RE)?;
    let (n_cols, n_rows) = toa_ds.raster_size();
    let idx_bounds = get_exterior_data_bounds( &toa_ds, 1)?.apply_margin( 10, n_cols, n_rows);

    // create cropped UTM (metric) toa dataset as basis for area computation
    crop_dataset( &toa_ds, &get_gtiff_driver(), &dir.join("toa.tif"), Some(get_gtiff_create_opts()), &idx_bounds)?;

    // note we neither scale (up-sample) nor apply an outlier filter to PNG colorization. Both would not work
    // well with nodata boundaries and we actually might want to give a visual feedback regarding the computational grid/precision

    //let scale_factor = if (idx_bounds.width * idx_bounds.height) < config.small_fire_cells { Some(2.0) } else { None }; // could be adaptive
    let scale_factor = None;
    let alg = ResampleAlg::NearestNeighbour;

    let ds = process_raster_file( toa_ds, &dir.join("toa_wgs84.tif"), &dir.join("toa.png"),
                                    &idx_bounds, &scale_factor, alg, &config.toa_colors, None)?; // no outliers here
    let geo_rect = get_geo_rect(&ds)?; // they are all the same so we have to get this just once
    raster_products.push( ("toa.png".into(), geo_rect.clone()));

    process_raster_file( get_ds( &dir, &FLIN_RE)?, &dir.join("flin_wgs84.tif"), &dir.join("flin.png"),
                                    &idx_bounds, &scale_factor, alg, &config.flin_colors, None)?;
    raster_products.push( ("flin.png".into(), geo_rect.clone()));

    process_raster_file( get_ds( &dir, &VS_RE)?, &dir.join("vs_wgs84.tif"), &dir.join("vs.png"),
                                    &idx_bounds, &scale_factor, alg, &config.vs_colors, None)?;
    raster_products.push( ("vs.png".into(), geo_rect.clone()));

    process_raster_file( get_ds( &dir, &FL_RE)?, &dir.join("fl_wgs84.tif"), &dir.join("fl.png"),
                                    &idx_bounds, &scale_factor, alg, &config.fl_colors, None)?;
    raster_products.push( ("fl.png".into(), geo_rect.clone()));

    process_raster_file( get_ds( &dir, &CF_RE)?, &dir.join("cf_wgs84.tif"), &dir.join("cf.png"),
                                   &idx_bounds, &scale_factor, alg, &config.cf_colors, None)?;
    raster_products.push( ("cf.png".into(), geo_rect));

    Ok( raster_products )
}

pub fn process_raster_file<P: AsRef<Path>> (
    ef_ds: Dataset,
    wgs84_path: P,
    png_path: P,
    idx_bounds: &IndexBounds2D,
    scale_factor: &Option<f64>,
    alg: ResampleAlg,
    cmap: &[u32],
    pct_range:Option<Range<f64>>
)->Result<Dataset>
{
    if wgs84_path.as_ref().is_file() { remove_file( &wgs84_path)? }
    if png_path.as_ref().is_file() { remove_file( &png_path)?; }

    let ds = crop_mem_dataset( &ef_ds, idx_bounds)?;
    let nodatas = vec![-9999.0; ds.raster_count()]; // one per band
    let mut ds = warp_to_wgs84_tif( &ds, &wgs84_path, nodatas)?;

    // up-sampling does not easily work with nodata boundaries. For interpolating resampling algorithms we would have to use
    // an approach like for contour generation, i.e. fill nodata regions with a value that pushes the interpolation into
    // a range we could easily identify during subsequent to_cmap translation

    if let Some(sf) = scale_factor {
        let (x_res, y_res) = get_resolution( &ds)?;
        ds = warp_to_res_tif( &ds, &wgs84_path, x_res / *sf, y_res / *sf, alg)?;
    }

    to_cmap( png_path, &ds, 1, pct_range, &cmap)?;
    Ok(ds)
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

/// we compute this ourselves so that we could extend it to get additional information such as growth per 30min interval
/// we can also control how holes are counted (e.g. water or developed)
pub fn compute_timesteps_from_toa<P: AsRef<Path>> (path: P, start: &DateTime<Utc>, levels: &[f64], cell_size: Area)->Result<Vec<Timestep>> {
    let ds = open_dataset(path)?;
    let mut vs: Vec<f32> = get_rasterband_as_vec( &ds, 1)?;
    let nodata: f32 = -9999.0;

    fn count_cells (vs: &[f32], level: f32, nodata: f32)->usize {
        let mut n: usize = 0;
        for i in 0..vs.len() {
            let v = vs[i];
            if v != nodata && v <= level { n += 1; }
        }
        n
    }

    let dt = minutes(30);
    let mut date = *start;
    let mut areas: Vec<Timestep> = Vec::with_capacity( levels.len());
    let mut prev: Area = Area::new::<acre>( 0.0);
    for t in levels.iter() {
        let n = count_cells( &vs, *t as f32, nodata);
        let area = cell_size * n as f64;
        date = date + dt;

        let growth: Ratio = Ratio::new::<ratio>(if prev.value == 0.0 { 0.0 } else { (area - prev).value / prev.value });
        prev = area;

        areas.push( Timestep { date, area, growth });
    }

    Ok( areas )
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
    use_barriers: bool,
}


pub fn get_utm_rect_around_point (center: &GeoPoint, dist: Length)->Result<UtmRect> {
    let mut p = geo_to_utm( center).ok_or( op_failed!("center not valid UTM"))?;
    p.round();
    let dm = dist.get::<meter>();

    let mut utm_rect = UtmRect::from_wsen_meters(p.easting - dm, p.northing - dm, p.easting + dm, p.northing + dm, p.utm_zone);
    utm_rect.round();

    Ok( utm_rect )
}

pub fn parse_area<P: AsRef<Path>> (path: P, base_date: DateTime<Utc>)->Result<Vec<Timestep>> {
    let mut vs: Vec<Timestep> = Vec::new();
    let mut ts_last = -100000.0;
    let mut prev: Area = Area::new::<acre>(0.0);

    let mut rdr = csv::Reader::from_reader( File::open(&path)?);
    for result in rdr.records() {
        let record = result?;
        if record.len() != 2 { return Err( op_failed!("not an Acres Record") ) }

        let ts: f64 = record[0].trim().parse()?; // this is in seconds
        if ts - ts_last > 300.0 { // only add new entries if they are more than 5min apart (Elmfire has a tendency to add some almost-duplicates at the tail)
            let acres: f64 = record[1].trim().parse()?;

            let date = base_date + Duration::from_secs_f64(ts);
            let area = Area::new::<acre>( acres);

            let growth: Ratio = Ratio::new::<ratio>(if prev.value == 0.0 { 0.0 } else { (area - prev).value / prev.value });
            prev = area;

            ts_last = ts;
            vs.push( Timestep { date, area, growth } )
        }
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

    pub fn get_json_remove_msg (&self)->String {
        let mut w = JsonWriter::with_capacity(128);
        w.write_object( |w| {
            w.write_field("id", &self.fire.id);
        });
        let msg = w.as_str();
        ws_msg_from_json( ElmfireService::mod_path(), "remove", &msg)
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

    /// use with caution - make sure we only delete valid Elmfire dirs in our CACHE_DIR
    pub fn remove_dir (&self)->Result<()> {
        let dir = &self.fire.dir;
        if dir.is_dir() {
            if let Some(parent) = dir.parent() {
                let cache_dir: &Path = &CACHE_DIR;
                if parent == cache_dir {
                    println!("removing Elmfire dir: {:?}", dir);
                    remove_dir_all( dir)?;
                    return Ok(())
                }
            }
        }

        Err( op_failed!( "cannot remove dir {:?}", dir) )
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

            w.write_array_field( "ignitions", |w| {
                for ignition in &fire.ignitions { ignition.write_json_to(w); }
            });

            // NOTE - no '\n' in JSON strings allowed, we have to escape/un-escape manually

            if let Some(fixed) = fire.fixed.as_ref() {
                w.write_json_field( "fixed", fixed); // object with value arrays in UI units
            }
            if let Some(spotting) = fire.spotting.as_ref() {
                w.write_field( "spotting", JsonEscapedStr(spotting))
            }

            w.write_array_field( "barriers", |w| {
                for barrier in &fire.barriers {
                    barrier.write_json_to(w);
                }
            });

            if let Some(result) = &self.result {
                match result {
                    Ok(outputs) => {
                        w.write_object_field( "output", |w|{
                            w.write_array_field("stats", |w|{
                                for s in outputs.stats.iter() {
                                    s.write_json_to(w);
                                }
                            });
                            w.write_array_field("timesteps", |w|{
                                for a in outputs.timesteps.iter() {
                                    a.write_json_to(w);
                                }
                            });
                            w.write_array_field("windfields", |w|{
                                for a in outputs.windfields.iter() {
                                    a.write_json_to(w);
                                }
                            });
                            w.write_json_field("compStats", &outputs.comp_stats);

                            w.write_array_field( "rasterProducts", |w|{
                                for (fname,geo_rect) in outputs.raster_products.iter() {
                                    w.write_object( |w| {
                                        w.write_field("file", fname);
                                        w.write_rect_field("bbox", geo_rect, NumFormat::Fp5);
                                        // TODO - extend with optional cmap
                                    })
                                }
                            });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ignitions_to_nml () {
        let base_date: DateTime<Utc> = DateTime::parse_from_rfc3339("2026-10-09T00:00:00Z").unwrap().to_utc();
        let ms = |s: &str| DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis(); // dates are sent as epoch millis
        let json = format!( r#"[
            {{"id":1, "ignitionType":"point", "date":{}, "value":{{"lon":-121.9021,"lat":37.1682}}}},
            {{"id":2, "ignitionType":"line", "date":{},
             "value":{{"points":[{{"lon":-121.95,"lat":37.15}},{{"lon":-121.93,"lat":37.16}},{{"lon":-121.91,"lat":37.16}}]}}}},
            {{"id":3, "ignitionType":"point", "date":{}, "value":{{"lon":-121.88,"lat":37.18}}}}
        ]"#, ms("2026-10-09T00:52:00Z"), ms("2026-10-09T01:30:00Z"), ms("2026-10-09T02:00:00Z"));
        let ignitions: Vec<Ignition> = serde_json::from_str( &json).unwrap();

        let geo_center = GeoPoint::from_lon_lat_degrees( -121.9021, 37.1682);
        let nml = ignitions_to_nml( base_date, &geo_center, &ignitions).unwrap();
        println!("{nml}");

        assert!( nml.starts_with("NUM_IGNITIONS = 2\n")); // only point ignitions
        assert!( nml.contains("T_IGN(1) = 3120\n"));     // 52min
        assert!( nml.contains("X_IGN(1) = 597473\n"));
        assert!( nml.contains("Y_IGN(1) = 4114096\n"));
        assert!( nml.contains("T_IGN(2) = 7200\n"));

        // 3 point line string -> 2 line ignitions with the same time
        assert!( nml.contains("T_LINE_IGN(1) = 5400\n"));
        assert!( nml.contains("T_LINE_IGN(2) = 5400\n"));
        assert!( !nml.contains("T_LINE_IGN(3)"));

        // legs are connected: end of leg 1 is start of leg 2
        let value = |key: &str| nml.lines().find( |l| l.starts_with(key)).unwrap().split('=').nth(1).unwrap().trim().to_string();
        assert_eq!( value("X_LINE_IGN_END(1)"), value("X_LINE_IGN_START(2)"));
        assert_eq!( value("Y_LINE_IGN_END(1)"), value("Y_LINE_IGN_START(2)"));

        // JsonWritable and serde (de)serialization use the same date format (epoch millis)
        let mut w = JsonWriter::new();
        ignitions[0].write_json_to( &mut w);
        let ign: Ignition = serde_json::from_str( w.as_str()).unwrap();
        assert_eq!( ign.date, ignitions[0].date);
        let ign: Ignition = serde_json::from_str( &serde_json::to_string( &ignitions[0]).unwrap()).unwrap();
        assert_eq!( ign.date, ignitions[0].date);
    }

    #[test]
    fn test_ignition_zone () {
        // -119.99 is in UTM zone 11 but the simulation center (-120.05) is in zone 10 - the ignition has to be in zone 10
        let base_date: DateTime<Utc> = DateTime::parse_from_rfc3339("2026-10-09T00:00:00Z").unwrap().to_utc();
        let date = base_date.timestamp_millis() + 600_000;
        let ignitions: Vec<Ignition> = serde_json::from_str( &format!(
            r#"[{{"id":1, "ignitionType":"point", "date":{date}, "value":{{"lon":-119.99,"lat":37.5}}}}]"#)).unwrap();

        let geo_center = GeoPoint::from_lon_lat_degrees( -120.05, 37.5);
        let nml = ignitions_to_nml( base_date, &geo_center, &ignitions).unwrap();
        println!("{nml}");

        let center = geo_to_utm( &geo_center).unwrap();
        let x: f64 = nml.lines().find( |l| l.starts_with("X_IGN(1)")).unwrap().split('=').nth(1).unwrap().trim().parse().unwrap();
        assert!( (x - center.easting).abs() < 10_000.0); // ~5km east of center in the same zone (not ~500km off)
    }

    #[test]
    fn test_datafile_ignitions () {
        let date: DateTime<Utc> = DateTime::parse_from_rfc3339("2026-10-09T00:52:00Z").unwrap().to_utc();
        let mut ef = ElmFire::from_point_ignition( "ignitionTest".to_string(), date, GeoPoint::from_lon_lat_degrees( -121.9021, 37.1682),
                                                  Length::new::<kilometer>(24.0), 4, false, None, None, Vec::new()).unwrap();
        let ls = GeoLineString::from_geo_points( vec![ GeoPoint::from_lon_lat_degrees(-121.95,37.15), GeoPoint::from_lon_lat_degrees(-121.93,37.16), GeoPoint::from_lon_lat_degrees(-121.91,37.16) ]);
        ef.ignitions.push( Ignition::new_line_string( 2, date + minutes(38), ls));

        let dir = std::env::temp_dir().join("odin_elmfire_datafile_test");
        let _ = std::fs::remove_dir_all( &dir);
        std::fs::create_dir_all( dir.join("inputs")).unwrap();
        ef.dir = dir.clone();

        let config: ElmfireConfig = load_config("elmfire.ron").unwrap();
        let path = ef.create_datafile( &config).unwrap();
        let data = std::fs::read_to_string( &path).unwrap();
        let _ = std::fs::remove_dir_all( &dir);

        let ign_section: Vec<&str> = data.lines().filter( |l| l.contains("IGN")).collect();
        println!("{}", ign_section.join("\n"));

        assert!( data.contains("NUM_IGNITIONS = 1\n"));
        assert!( data.contains("T_IGN(1) = 3120\n"));   // 52min after base date
        assert!( data.contains("X_IGN(1) = 597473\n"));
        assert!( data.contains("Y_IGN(1) = 4114096\n"));
        assert!( data.contains("T_LINE_IGN(1) = 5400\n")); // 52+38min
        assert!( data.contains("T_LINE_IGN(2) = 5400\n"));

        // client JSON contains the ignitions as Ignition objects
        let run = ElmfireRun{ fire: Arc::new(ef), result: None };
        let mut w = JsonWriter::new();
        run.write_json_to( &mut w);
        let json: serde_json::Value = serde_json::from_str( w.as_str()).unwrap();
        let igns = json["ignitions"].as_array().unwrap();
        assert_eq!( igns.len(), 2);
        assert_eq!( igns[0]["ignitionType"], "point");
        assert_eq!( igns[1]["value"]["points"].as_array().unwrap().len(), 3);

        // and they deserialize back into Ignitions (as sent by the client in ElmfireRequests)
        let igns1: Vec<Ignition> = serde_json::from_value( json["ignitions"].clone()).unwrap();
        assert_eq!( igns1.len(), 2);
    }
}
