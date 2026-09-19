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

use std::{fs::File, io::{Read,Write}, marker::PhantomData, path::{Path,PathBuf}, process::Output};
use reqwest::{Client};
use csv;
use serde::{Serialize,Deserialize};
use serde_json;
use lazy_static::lazy_static;
use chrono::{DateTime,Utc,TimeZone};
use askama::Template;

use uom::si::{angle::degree, f64::{Angle, HeatFluxDensity, Length, Pressure, Ratio, ThermodynamicTemperature, Velocity}, length::millimeter, time::hour};
use uom::si::{
    thermodynamic_temperature::{degree_celsius,degree_fahrenheit},
    pressure::{pascal,inch_of_mercury},
    length::{inch,meter},
    velocity::{mile_per_hour,meter_per_second},
    ratio::{percent,ratio},
    heat_flux_density::watt_per_square_meter,
};

use odin_build::{define_load_config, pkg_cache_dir, pkg_data_dir};
use odin_gdal::{geods::{GeoDs,GeoDsSrs}};
use odin_macro::public_struct;
use odin_common::{
    run_captured_cmd,
    datetime::{self, days, get_offset_hours, as_hours, ymd, duration_since, is_between_inclusive},
    fs::{EnvPathBuf, path_as_str},
    geo::GeoPoint,
    net::get_deserialized_cached_json_response
};
use odin_landfire::{evt_cats::{EVT_CATEGORIES, EvtCategories}, get_evt_value_at_geo_point};
use odin_openmeteo::{
    DailyLocationData, DataSet, HourlyLocationData, LocationData, OpenMeteoConfig, VecDataSet,
    dataset::{Fw21Data, PrecipitationSum},
    get_archive_url, get_forecast_url, queries::get_annual_precipitation,
    ids::{FieldId::relative_humidity_2m, ModelId}
};

pub mod errors;
use errors::{OdinNfdrsError,Result,op_failed};

define_load_config!();

lazy_static! {
    pub static ref CACHE_DIR: PathBuf = { pkg_cache_dir!() };
}

#[derive(Deserialize,Debug)]
#[public_struct]
struct NfdrsConfig {
    nfdrs_cmd: EnvPathBuf,  // path to NFDRS4_cli command (absolute or in $PATH)
}

// DateTime, Temperature(F), RelativeHumidity(%), Precipitation(in), WindSpeed(mph), WindAzimuth(degrees), SolarRadiation(W/m2), SnowFlag
// 2001-03-22T15:00:00-07:00, 62, 16, 0.000, 4, 6, 650,1

#[derive(Debug)]
#[public_struct]
struct Fw21Record {
    date_time: DateTime<Utc>,
    temperature: ThermodynamicTemperature,
    relative_humidity: Ratio,
    precipitation: Length,
    wind_speed: Velocity,
    wind_azimuth: Angle,
    wind_gusts: Velocity,
    solar_radiation: HeatFluxDensity,
    snow_flag: bool
}

impl Fw21Record {

    /// TODO - remove
    pub fn from_time_step (hourly_data: &HourlyLocationData<Fw21Data>, i: usize)->Result<Self> {
        let hourly = &hourly_data.hourly;
        let date_time = hourly.nth_time_step( i)?; // get this first since it ensures subsequent index ops will not fail

        let temperature = ThermodynamicTemperature::new::<degree_celsius>(hourly.temperature_2m[i].get::<degree_fahrenheit>() as f64);
        let relative_humidity = Ratio::new::<percent>( hourly.relative_humidity_2m[i].get::<percent>() as f64);
        let precipitation = Length::new::<millimeter>( hourly.precipitation[i].get::<inch>() as f64);
        let wind_speed = Velocity::new::<meter_per_second>( hourly.wind_speed_10m[i].get::<mile_per_hour>() as f64);
        let wind_azimuth = Angle::new::<degree>( hourly.wind_direction_10m[i].get::<degree>() as f64);
        let wind_gusts = Velocity::new::<meter_per_second>( hourly.wind_gusts_10m[i].get::<mile_per_hour>() as f64);
        let solar_radiation = HeatFluxDensity::new::<watt_per_square_meter>( hourly.shortwave_radiation[i].get::<watt_per_square_meter>() as f64);  // TODO is this the right one?
        let snow_flag = hourly.snow_depth[i].value > 0.0;

        Ok( Fw21Record{date_time,temperature,relative_humidity,precipitation,wind_speed,wind_azimuth,wind_gusts,solar_radiation,snow_flag} )
    }

    /// convert time step i to Fw21Record (involves conversion of basetype f32 -> f64)
    pub fn for_step (fw21_data: &Fw21Data, i: usize)->Result<Self> {
        if i >= fw21_data.time.len() {
            Err( op_failed!("time step outside range") )
        } else {
            let date_time = fw21_data.time[i];
            // note this works since uom internally converts to same base unit
            let temperature = ThermodynamicTemperature { value: fw21_data.temperature_2m[i].value as f64, dimension: PhantomData, units: PhantomData };
            let relative_humidity = Ratio { value: fw21_data.relative_humidity_2m[i].value as f64, dimension: PhantomData, units: PhantomData };
            let precipitation = Length { value: fw21_data.precipitation[i].value as f64, dimension: PhantomData, units: PhantomData };
            let wind_speed = Velocity { value: fw21_data.wind_speed_10m[i].value as f64, dimension: PhantomData, units: PhantomData };
            let wind_azimuth = Angle { value: fw21_data.wind_direction_10m[i].value as f64, dimension: PhantomData, units: PhantomData };
            let wind_gusts = Velocity { value: fw21_data.wind_gusts_10m[i].value as f64, dimension: PhantomData, units: PhantomData };
            let solar_radiation = HeatFluxDensity { value: fw21_data.shortwave_radiation[i].value as f64, dimension: PhantomData, units: PhantomData };
            let snow_flag = fw21_data.snow_depth[i].value > 0.0;

            Ok( Fw21Record{date_time,temperature,relative_humidity,precipitation,wind_speed,wind_azimuth,wind_gusts,solar_radiation,snow_flag} )
        }
    }

    pub fn recs_from_fw21_data (fw21_data: &Fw21Data)->Result<Vec<Fw21Record>> {
        let n_hours = fw21_data.n_time_steps();
        let mut rs: Vec<Fw21Record> = Vec::with_capacity( n_hours);
        for i in 0..n_hours {
            rs.push( Self::for_step( fw21_data, i)?)
        }
        Ok( rs )
    }

    // DateTime, Temperature(F), RelativeHumidity(%), Precipitation(in), WindSpeed(mph), WindAzimuth(degrees), SolarRadiation(W/m2), SnowFlag
    // 2001-03-22T15:00:00-07:00, 62, 16, 0.000, 4, 6, 650, 1
    // ...
    pub fn save_as_csv<P: AsRef<Path>, Z:TimeZone> (path: P, vs: &[Fw21Record], tz: &Z)->Result<()> {
        let mut file = File::create(path)?;
        writeln!(file, "DateTime, Temperature(F), RelativeHumidity(%), Precipitation(in), WindSpeed(mph), WindAzimuth(degrees), GustSpeed(mph), SolarRadiation(W/m2), SnowFlag");
        for v in vs {
            writeln!(file, "{},{:.0},{:.0},{:.1},{:.1},{:.0},{:.1},{:.0},{}",
                v.date_time.with_timezone(tz).to_rfc3339(),
                v.temperature.get::<degree_fahrenheit>(),
                v.relative_humidity.get::<percent>(),
                v.precipitation.get::<inch>(),
                v.wind_speed.get::<mile_per_hour>(),
                v.wind_azimuth.get::<degree>(),
                v.wind_gusts.get::<mile_per_hour>(),
                v.solar_radiation.get::<watt_per_square_meter>(),
                if v.snow_flag { 1 } else { 0 }
            )?
        }
        Ok(())
    }
}

/// NFDRS4 result data (full)
#[derive(Serialize,Deserialize,Debug)]
#[public_struct]
struct NfdrsRecord {
    station_id: String,
    date: DateTime<Utc>,
    temperature: ThermodynamicTemperature,
    relative_humidity: Ratio,
    precipitation: Length,
    wind_speed: Velocity,
    wind_azimuth: Angle,
    solar_radiation: HeatFluxDensity,
    snow_flag: bool,
    gust_speed: Option<Velocity>,
    gust_azimuth: Option<Angle>,
    dfm_1h: Ratio,
    dfm_10h: Ratio,
    dfm_100h: Ratio,
    dfm_1000h: Ratio,
    lfm_herb: Ratio,
    lfm_woody: Ratio,
    fuel_temp: ThermodynamicTemperature,
    bi: f32,
    erc: f32,
    sc: f32,
    ic: f32,
    gsi: f32,
    kbdi: u32
}

/// our main computation entry that assumes all metereological data has been already acquired by the caller
/// and we only have to transform into the right units/formats required to run NFDRS
/// NOTE - our Fw21Data has to include past time points (>100h) to account for recent moisture changes but we
/// only want records since base_date
pub async fn compute_nfdrs_recs (
    config: &NfdrsConfig,
    pos: &GeoPoint,
    fw21: &Fw21Data,
    annual_precip: Length,
    loc_cls: &NfdrsLocationClassification,
    model: ModelId,
    base_date: &DateTime<Utc>
)->Result<Vec<NfdrsRecord>>
{
    let times = &fw21.times();
    if times.is_empty() { return Err( op_failed!("no time steps in FW21 data")) }
    let start = &times[0];
    let end = &times[times.len()-1];

    let output_path = output_path( pos, start, end, model);

    let lat = pos.latitude_degrees();
    let tz = datetime::get_tz_of_location( &pos).ok_or( op_failed!("invalid location"))?;
    let tz_off = get_offset_hours( &tz) as i32;
    let lat = pos.latitude_degrees();

    let avg_precip_inch = annual_precip.get::<inch>();

    let fw21_path = fw21_path( &pos, start, end, model);
    let fw21_recs = Fw21Record::recs_from_fw21_data( fw21)?; // if this fails we have inconsistent input data
    Fw21Record::save_as_csv( &fw21_path, &fw21_recs, &tz);

    //--- create the NFDRS cfg and init files
    let id = nfdrs_id ( &pos);
    let cfg_path = cfg_path( &pos, start, end, model);
    let init_path = init_path( &pos);
    //let output_path = output_path( pos, &date, forecast_days, past_days, wx_model);
    let max_sc = 3; // ?? where does that come from

    create_nfdrs_cfg_file( &cfg_path, &id, &fw21_path, &init_path, &output_path)?;
    create_nfdrs_init_file( &init_path, lat, loc_cls.fuel_model, loc_cls.slope_flag, avg_precip_inch, loc_cls.fuel_flag, tz_off, max_sc)?;

    run_nfdrs( &config.nfdrs_cmd, &cfg_path).await?;
    let nfdrs_recs = parse_nfdrs_records( &output_path, |rec| &rec.date >= base_date)?;

    Ok( nfdrs_recs )
}

pub async fn run_nfdrs<P: AsRef<Path>> (cmd: &EnvPathBuf, cfg_path: P)->Result<()> {
    let output: Output = run_captured_cmd!( cmd, path_as_str( &cfg_path)? ).await?;
    let exit_status = output.status;
    if exit_status.success() {
        Ok(())
    } else {
        Err( op_failed!("NFDRS command failed with exit status {:?}", exit_status.code()))
    }
}

pub fn parse_nfdrs_records<P,F> (path: P, filter: F)->Result<Vec<NfdrsRecord>>
    where P: AsRef<Path>, F: Fn(&NfdrsRecord)->bool
{
    let mut rdr = csv::Reader::from_reader( File::open(path)?);
    let mut rs: Vec<NfdrsRecord> = Vec::new();

    for result in rdr.records() {
        let record = result?;
        if record.len() != 24 { return Err( op_failed!("not a NfdrsRecord") ) }

        let station_id = record[0].to_string();
        let date = DateTime::parse_from_str( &record[1], "%Y%m%dT%H%M%S%z")?.to_utc();
        let temperature = ThermodynamicTemperature::new::<degree_fahrenheit>( record[2].parse::<f64>()?);
        let relative_humidity = Ratio::new::<percent>( record[3].parse::<f64>()?);
        let precipitation = Length::new::<inch>( record[4].parse::<f64>()?);
        let wind_speed = Velocity::new::<mile_per_hour>( record[5].parse::<f64>()?);
        let wind_azimuth = Angle::new::<degree>( record[6].parse::<f64>()?);
        let solar_radiation = HeatFluxDensity::new::<watt_per_square_meter>( record[7].parse::<f64>()?);
        let snow_flag = &record[8] == "1";

        let gust_speed = if &record[9] == "-999.0" { None } else {
            Some( Velocity::new::<mile_per_hour>( record[9].parse::<f64>()?) )
        };
        let gust_azimuth = if &record[10] == "-999" { None } else {
            Some( Angle::new::<degree>( record[10].parse::<f64>()?) )
        };
        let dfm_1h = Ratio::new::<percent>( record[11].parse::<f64>()?);
        let dfm_10h = Ratio::new::<percent>( record[12].parse::<f64>()?);
        let dfm_100h = Ratio::new::<percent>( record[13].parse::<f64>()?);
        let dfm_1000h = Ratio::new::<percent>( record[14].parse::<f64>()?);

        let lfm_herb = Ratio::new::<percent>( record[15].parse::<f64>()?);
        let lfm_woody = Ratio::new::<percent>( record[16].parse::<f64>()?);

        let fuel_temp = ThermodynamicTemperature::new::<degree_celsius>( record[17].parse::<f64>()?); // note NFDRS4 reports it as celsius

        let bi = record[18].parse::<f32>()?;
        let erc = record[19].parse::<f32>()?;
        let sc = record[20].parse::<f32>()?;
        let ic = record[21].parse::<f32>()?;
        let gsi = record[22].parse::<f32>()?;
        let kbdi = record[23].parse::<u32>()?;

        let r = NfdrsRecord {
            station_id, date, temperature, relative_humidity, precipitation,
            wind_speed, wind_azimuth, solar_radiation, snow_flag, gust_speed, gust_azimuth,
            dfm_1h, dfm_10h, dfm_100h, dfm_1000h, lfm_herb, lfm_woody, fuel_temp,
            bi, erc, sc, ic, gsi, kbdi
        };

        if filter( &r) {
            rs.push(r)
        }
    }

    Ok(rs)
}

#[derive(Debug)]
#[public_struct]
struct NfdrsLocationClassification {
    slope_flag: u8,
    fuel_flag: u8,
    fuel_model: &'static str
}

impl NfdrsLocationClassification {

    pub async fn for_location (lf_src: &GeoDs, pos: &GeoPoint)->Result<Self> {
        let slope_flag = get_slope_class( lf_src, pos).await?;
        //let slope_flag = 0; // this would turn off index calculation (silently set to 0.0 in output)

        let (fuel_flag,fuel_model) = get_fuel_cats( lf_src, pos).await?;
        //let (fuel_flag, fuel_model) = (0,"Y");

        Ok( Self { slope_flag, fuel_flag, fuel_model } )
    }
}

/// standard NFDRS Slope Class (1 = 0.25%, 2 = 26-40%, 3 = 41 - 55%, 4 = 56 - 75%, 5 = 76%+)
pub async fn get_slope_class (lf_src: &GeoDs, pos: &GeoPoint)->Result<u8> {
    let srs = GeoDsSrs::from_epsg(4326).unwrap(); // -> locations are lat,lon
    let mut locations = vec![ (pos.latitude_degrees(), pos.longitude_degrees()) ];
    let nodata: f64 = -9999.00;

    let vs = lf_src.get_values( "SlpD", Some(-9999.00), &srs, &mut locations).await?;

    if vs.len() != 1 { return Err( op_failed!("invalid response length")) }
    let slope = vs[0];
    if slope == nodata { return Err( op_failed!("no slope data at position {:?}", pos)) }

    // phi = atan(ratio)

    if slope < 0.14 { return Ok(1) }
    if slope < 21.8 { return Ok(2) }
    if slope < 28.8 { return Ok(3) }
    if slope < 36.9 { return Ok(4) }
    Ok (5)
}

/// 0 for perennials, 1 for herbs/annuals
pub async fn get_fuel_cats (lf_src: &GeoDs, pos: &GeoPoint)->Result<(u8,&'static str)> {
    let v = get_evt_value_at_geo_point(lf_src, pos).await?;

    let fuel_flag = if EVT_CATEGORIES.is_herb(v) || EVT_CATEGORIES.is_agriculture(v) {
        1
    } else if EVT_CATEGORIES.is_shrub(v) || EVT_CATEGORIES.is_tree(v) {
        0
    } else {
        0 // we just treat it as perennial
        //return Err( op_failed!("not a known NFDRS fuel type: {} at {}", v, pos) ) // bail - no vegetation at pos
    };

    let fuel_model = if EVT_CATEGORIES.is_shrub_dominated(v) {
        "X"  // TODO - what about "W" (grass-shrub)
    } else if EVT_CATEGORIES.is_herb_dominated(v) {
        "V"
    } else {
        "Y" // we fall back to tree-dominated
    };

    Ok( (fuel_flag, fuel_model))
}

pub async fn get_fw21_records (config: &OpenMeteoConfig, pos: &GeoPoint, start: &DateTime<Utc>, end: &DateTime<Utc>, model: ModelId)->Result<Vec<Fw21Record>> {
    let date = Utc::now();
    let fname = format!("fw21__{}__{}.json", pos.encode(3), ymd( &date));
    let path = CACHE_DIR.join( fname);

    let url = get_forecast_url( config, &HourlyLocationData::<Fw21Data>::get_point_query( model, start, end, pos));
    let ld: HourlyLocationData<Fw21Data> = get_deserialized_cached_json_response( &url, &path).await?;

    let n_hours = ld.hourly.n_time_steps();
    let mut rs: Vec<Fw21Record> = Vec::with_capacity( n_hours);

    for i in 0..n_hours {
        rs.push( Fw21Record::from_time_step( &ld, i)?)
    }

    Ok(rs)
}

// since only the NFDRS configuration file needs to be provided (it contains the wx and init paths) it is enough to return this path
pub async fn create_nfdrs_inputs (
    config: &OpenMeteoConfig,
    lf_src: &GeoDs,
    pos: &GeoPoint,
    start: &DateTime<Utc>,
    end: &DateTime<Utc>,
    model: ModelId,
    output_path: &PathBuf
) -> Result<PathBuf>
{
    //--- make sure this is a location for which we can calculate NFDRS records (has wildland fuels)
    let tz = datetime::get_tz_of_location( pos).ok_or( op_failed!("invalid location"))?;
    let tz_off = get_offset_hours( &tz) as i32;
    let lat = pos.latitude_degrees();

    let (annual_fuel_flag, fuel_model) = get_fuel_cats( lf_src, pos).await?; // this will bail if there is no vegetation at pos
    let slope_class = get_slope_class( lf_src, pos).await?;

    //--- get and save the wx data as FW21 records the NFDRS executable expects
    let fw21_recs = get_fw21_records( config, pos, start, end, model).await?;
    let fw21_path = fw21_path( pos, start, end, model);
    Fw21Record::save_as_csv( &fw21_path, &fw21_recs, &tz);

    let avg_precip = get_annual_precipitation( config, pos, start).await?;
    let avg_precip_inch = avg_precip.get::<inch>() as f64;

    //--- create the NFDRS cfg and init files
    let id = nfdrs_id (pos);
    let cfg_path = cfg_path( pos, start, end, model);
    let init_path = init_path(pos);
    //let output_path = output_path( pos, &date, forecast_days, past_days, wx_model);
    let max_sc = 3; // ?? where does that come from

    create_nfdrs_cfg_file( &cfg_path, &id, &fw21_path, &init_path, output_path)?;
    create_nfdrs_init_file( &init_path, lat, fuel_model, slope_class, avg_precip_inch, annual_fuel_flag, tz_off, max_sc)?;

    Ok( cfg_path )
}

fn nfdrs_id (pos: &GeoPoint)->String {
    format!("@__{}", pos.encode(3))
}

fn fw21_path (pos: &GeoPoint, start: &DateTime<Utc>, end: &DateTime<Utc>, model: ModelId)->PathBuf {
    let fname = format!("fw21__{}__{}-{}__{}.csv", pos.encode(3), ymd( &start), as_hours(duration_since(end,start)), model.as_ref());
    CACHE_DIR.join( fname)
}

fn cfg_path (pos: &GeoPoint, start: &DateTime<Utc>, end: &DateTime<Utc>, model: ModelId)->PathBuf {
    let fname = format!("nfdrs_conf__{}__{}-{}__{}.cfg", pos.encode(3), ymd( &start), as_hours(duration_since(end,start)), model.as_ref());
    CACHE_DIR.join( fname)
}

fn init_path (pos: &GeoPoint)->PathBuf {
    let fname = format!("nfdrs_init__{}.cfg", pos.encode(3));
    CACHE_DIR.join( fname)
}

pub fn output_path (pos: &GeoPoint, start: &DateTime<Utc>, end: &DateTime<Utc>, model: ModelId)->PathBuf {
    let fname = format!("nfdrs_output__{}__{}-{}__{}.csv", pos.encode(3), ymd( &start), as_hours(duration_since(end,start)), model.as_ref());
    CACHE_DIR.join( fname)
}


#[derive(Template)]
#[template( path = "nfdrs_config.askama")]
struct CfgTemplate<'a> {
    station_id: &'a str,
    init_path: &'a str,
    fw21_path: &'a str,
    nfdrs_path: &'a str,
}

/// as specified in https://github.com/firelab/NFDRS4/blob/master/doc/NFDRS4_cli_Configuration_File.pdf
/// see also https://github.com/firelab/NFDRS4-TechDoc
fn create_nfdrs_cfg_file<P> (cfg_path: P, id: &str, fw21_path: P, init_path: P, output_path: P)->Result<()>
    where P: AsRef<Path>
{
    let contents = CfgTemplate {
        station_id: id,
        init_path: path_as_str( &init_path)?,
        fw21_path: path_as_str( &fw21_path)?,
        nfdrs_path: path_as_str( &output_path)?
    }.to_string();

    std::fs::write( cfg_path, contents.as_str())?;
    Ok(())
}


#[derive(Template)]
#[template( path = "nfdrs_init.askama")]
struct InitTemplate<'a> {
    lat_degrees: f64,
    fuel_model: &'a str,
    slope_class: u8,
    avg_annual_precip_inch: f64,
    annual_flag: u8,
    tz_offset: i32,
    max_sc: u32
}

/// as specified in https://github.com/firelab/NFDRS4/blob/master/doc/NFDRS4_Init_Configuration_File.pdf
/// see also https://github.com/firelab/NFDRS4-TechDoc
pub fn create_nfdrs_init_file<P> (
    init_path: P,
    lat_degrees: f64,
    fuel_model: &str,
    slope_class: u8,
    avg_annual_precip_inch: f64,
    annual_flag: u8,
    tz_offset: i32,
    max_sc: u32
)->Result<()>
    where P: AsRef<Path>
{
    let contents = InitTemplate { lat_degrees, fuel_model, slope_class, avg_annual_precip_inch, annual_flag, tz_offset, max_sc }.to_string();
    std::fs::write( init_path, contents.as_str())?;
    Ok(())
}
