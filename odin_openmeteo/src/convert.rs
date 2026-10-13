/*
 * Copyright © 2026, United States Government, as represented by the Administrator of
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
use std::{path::{Path,PathBuf}, sync::Arc, ops::Range};
use gdal::{Metadata, raster::RasterCreationOptions, Dataset as GdalDataset};
use chrono::{DateTime,Utc};
use serde::Deserialize;
use ndarray::prelude::*;

use uom::si::{
    f32::{ThermodynamicTemperature as Temperature32, Pressure as Pressure32, Ratio as Ratio32,
          Length as Length32, Velocity as Velocity32, Angle as Angle32, HeatFluxDensity as HeatFluxDensity32},
    thermodynamic_temperature::{degree_celsius,degree_fahrenheit},
    pressure::{hectopascal,pascal},
    velocity::{kilometer_per_hour,meter_per_second,mile_per_hour},
    length::{millimeter,centimeter, meter, inch},
    angle::{degree,radian},
    ratio::{percent,ratio},
    heat_flux_density::{watt_per_square_meter}
};

use odin_common::{utm::UtmRect, geo::GeoRect, fs::odin_data_filename, num_type::NumericType, datetime::hours};
use odin_gdal::{
    get_driver_for_extension, get_driver_by_name, get_gtiff_create_opts, get_gtiff_driver, CslStringList, set_band_meta, GdalType, is_gtiff_driver,
    errors::OdinGdalError,
    geods::{GeoDs, GeoDsImgType, GeoDsSrs},
    grid::{self, GdalGridAlgorithmOptions, create_grid_ds_from_scatter, create_grid_ds_from_raster}
};
use odin_wx::WxDataSetRequest;
use odin_hrrr::{meta, op_failed};

use crate::{
    HourlyLocationData, LocationData, OpenMeteoTemporal, Result, DataSet, VecDataSet, parse_location_data, start_step,
    grid::UtmGrid,
    dataset::{BasicData, MinimalSfcData, WindNinjaDataSet},
    errors::op_failed,
};

/// re-grid an Open-Meteo dataset (represented as a `Vec<HourlyLocationData<T>>`) into an equidistant
/// geotiff grid with bands that correspond to variables/meta info according to HRRR forecasts
pub fn wn_path_to_hrrr<P,U,T> (region: &str, model_name: &str, bbox: &GeoRect, path: P, tgt_dir: U)->Result<Vec<Arc<PathBuf>>>
    where P: AsRef<Path>, U: AsRef<Path>, T: WindNinjaDataSet + for<'de> Deserialize<'de>
{
    let data: Vec<HourlyLocationData<T>> = parse_location_data( path)?;
    wn_dataset_to_hrrr( region, model_name, bbox, &data, tgt_dir)
}

pub fn wn_path_to_filtered_hrrr<P,U,T> (region: &str, model_name: &str, bbox: &GeoRect, start: &DateTime<Utc>, end: &DateTime<Utc>, path: P, tgt_dir: U)->Result<Vec<Arc<PathBuf>>>
    where P: AsRef<Path>, U: AsRef<Path>, T: WindNinjaDataSet + for<'de> Deserialize<'de>
{
    let mut data: Vec<HourlyLocationData<T>> = parse_location_data( path)?;
    for hld in data.iter_mut() {
        hld.hourly.filter_time_steps( start, end); // this drops everything outside the given time interval
    }

    wn_dataset_to_hrrr( region, model_name, bbox, &data, tgt_dir)
}

/// re-grid an Open-Meteo dataset (represented as a `Vec<HourlyLocationData<T>>`) into an equidistant
/// geotiff grid with bands that correspond to variables/meta info according to HRRR forecasts
pub fn wn_dataset_to_hrrr<P,T> (region: &str, model_name: &str, bbox: &GeoRect, data: &[HourlyLocationData<T>], tgt_dir: P)->Result<Vec<Arc<PathBuf>>>
    where P: AsRef<Path>, T: WindNinjaDataSet
{
    if data.is_empty() {
        Ok( Vec::with_capacity(0) )
    } else {
        let alg = GdalGridAlgorithmOptions::nearest_neighbor_within( 0.2, -9999.0);
        let ref_data = &data[0];
        let n_time_steps = ref_data.hourly.n_time_steps();
        let basedate = ref_data.hourly.times()[0];
        let n_bands = if ref_data.hourly.wind_speed_100m().is_some() { 6 } else { 4 };

        let co = RasterCreationOptions::from_iter(["COMPRESS=DEFLATE", "PREDICTOR=2"]);
        let mut res = create_hourly_geo_ds::<T,_,f32,_>(
            data,
            0..n_time_steps,
            &alg,
            tgt_dir, region, model_name, T::dataset_name(), "tif" ,
            &co, n_bands, bbox, 0.1, 0.1, push_hrrr_timestep
        )?;

        let mut paths: Vec<Arc<PathBuf>> = Vec::new();
        for ds in res.iter_mut() {
            let fc_epoch = ds.metadata_item("forecast_epoch", "").unwrap();
            set_hrrr_metadata(ds, n_bands, &fc_epoch)?;
            let fname = ds.description()?;
            paths.push( Arc::new( Path::new(&fname).to_path_buf()) );
        }

        Ok( paths )
    }
}

// make sure we convert to the units HRRR uses (see respective GRIB_UNIT metadata entries of HRRR generated grib2 files)
fn push_hrrr_timestep<T,U> (d: &T, ts: usize, tgt: &mut Vec<Vec<U>>) where T: WindNinjaDataSet, U: From<f32> {
    let ws10 = d.wind_speed_10m()[ts].get::<meter_per_second>();
    let wd10 = d.wind_direction_10m()[ts].get::<radian>(); // meteorological convention: direction wind is coming from

    tgt[0].push( d.temperature_2m()[ts].get::<degree_celsius>().into());
    tgt[1].push( d.cloud_cover()[ts].get::<percent>().into());
    tgt[2].push( (-ws10 * wd10.sin()).into());
    tgt[3].push( (-ws10 * wd10.cos()).into());

    if let Some( wind_speed_80m ) = d.wind_speed_100m()  // note OpenMeteo only has 10,100m winds
    && let Some( wind_direction_80m ) = d.wind_direction_100m() {
        let ws100 = wind_speed_80m[ts].get::<meter_per_second>();
        let wd100 = wind_direction_80m[ts].get::<radian>();
        tgt[4].push( (-ws100 * wd100.sin()).into());
        tgt[5].push( (-ws100 * wd100.cos()).into());
    }
}

pub fn set_hrrr_metadata (ds: &mut GdalDataset, n_bands: usize, fc_epoch: &str)->Result<()> {
    set_band_meta( ds, 1, meta::TMP_2_HTGL_GRIB_META);
    set_band_meta( ds, 2, meta::TCDC_0_EATM_GRIB_META);
    set_band_meta( ds, 3, meta::UGRD_10_HTGL_GRIB_META);
    set_band_meta( ds, 4, meta::VGRD_10_HTGL_GRIB_META);

    if n_bands == 6 {
        set_band_meta( ds, 5, meta::UGRD_80_HTGL_GRIB_META);  // should be 100
        set_band_meta( ds, 6, meta::VGRD_80_HTGL_GRIB_META);  // ditto
    }

    // TODO - not sure we need those
    let dt_grib_meta = &[
        ("GRIB_REF_TIME", fc_epoch, ""),
        ("GRIB_VALID_TIME", fc_epoch, ""),
        ("GRIB_FORECAST_SECONDS", "0", "")
    ];

    set_band_meta( ds, 1, dt_grib_meta);
    set_band_meta( ds, 2, dt_grib_meta);
    set_band_meta( ds, 3, dt_grib_meta);
    set_band_meta( ds, 4, dt_grib_meta);

    if n_bands == 6 {
        set_band_meta( ds, 5, dt_grib_meta);
        set_band_meta( ds, 6, dt_grib_meta);
    }

    Ok(())
}

/// the Dataset metainfo key for the forecast time
pub const FORECAST_EPOCH: &'static str = "forecast_epoch";

/// the Dataset metainfo key for the forecast time step (usually number of hours since base time)
pub const FORECAST_STEP: &'static str = "forecast_step";

/// turn a set of homogenous HourlyLocationData records into a list of GDAL Datasets with one instance/file per
/// time point, using an EPSG 4326 SRS.
/// This essentially creates geographic grids with single time points out of a list of locations that all
/// have the same DataSet type with the same list of time points
pub fn create_hourly_geo_ds<T,P,U,F> (
    data: &[HourlyLocationData<T>],
    time_interval: Range<usize>,
    alg: &GdalGridAlgorithmOptions,
    tgt_dir: P,
    region: &str,
    model_name: &str,
    dataset_name: &str,
    file_ext: &str,
    create_opts: &RasterCreationOptions,
    n_tgt_bands: usize, // we might need additional ones for computed values
    bbox: &GeoRect,
    x_res: f64,
    y_res: f64,
    mut push_timestep: F
)->Result<Vec<GdalDataset>>
    where
        T: DataSet + VecDataSet,
        P: AsRef<Path>,
        U: GdalType + NumericType,
        F: FnMut(&T,usize,&mut Vec<Vec<f64>>),
{
    if data.is_empty() { return Ok( Vec::with_capacity(0)) }
    let ref_data = &data[0].hourly;

    let time_steps = ref_data.times();
    if time_steps.is_empty() { return Ok( Vec::with_capacity(0)) }
    let basedate = time_steps[0]; // TODO - is this always true or should we pass this in?

    let raster_driver = get_driver_for_extension(file_ext).ok_or( op_failed!("unsupported raster driver"))?;

    let [west,south,east,north] = bbox.to_wsen_degrees();

    let x_size = ((east - west) / x_res) as usize;
    let y_size = ((north - south) / y_res) as usize;

    let x_coords: Vec<f64> = data.iter().map( |d| d.meta_data.longitude as f64).collect();
    let y_coords: Vec<f64> = data.iter().map( |d| d.meta_data.latitude as f64).collect();

    let n_pos = x_coords.len();

    // the intermediate representation of values
    let mut tgt_vs: Vec<Vec<f64>> = vec![ Vec::with_capacity(n_pos); n_tgt_bands];
    let mut ts = 0;
    let mut result = Vec::new();

    for date in time_steps[time_interval].iter() {
        for v in &mut tgt_vs { v.clear(); }

        for d in data {
            push_timestep( &d.hourly, ts, &mut tgt_vs); // the passed-in closure that pushes the fields we are interested in
        }

        let step = (*date - basedate).num_hours().to_string();
        let fname = odin_data_filename( region, Some(basedate), &[ &step, model_name, dataset_name], Some(file_ext));
        let path = tgt_dir.as_ref().to_path_buf().join( fname);
        let create_opts = if is_gtiff_driver(&raster_driver) {
            RasterCreationOptions::from_iter( ["COMPRESS=DEFLATE", "PREDICTOR=2"])
        } else {
            RasterCreationOptions::new()
        };

        let mut ds = create_grid_ds_from_scatter::<U,_>(
            &raster_driver, &path,
            4326, west, east, south, north,
            x_size, y_size,
            &x_coords, &y_coords,
            &tgt_vs,
            alg,
            &create_opts
        )?;

        // GDAL automatically sets the filename as the description metainfo
        // we do add the forecast date here
        //ds.set_description( &path);
        ds.set_metadata_item( FORECAST_EPOCH, &date.timestamp().to_string(), "");
        ds.set_metadata_item( FORECAST_STEP, &ts.to_string(), "");

        result.push(ds);
        ts += 1;
    }

    Ok( result )
}

/// create a GDAL dataset with stacked hourly bands from an Array2<HourlyLocationData<T>> grid using the supplied band value extractor function
pub fn create_stacked_hourly_utm_ds<T,P,U,F> (
    path: P,
    copts: &RasterCreationOptions,
    utm_rect: &UtmRect,
    x_res: f64,
    y_res: f64,
    time_interval: Range<usize>,
    nodata_value: U,
    grid_data: &Array2<Option<T>>,
    mut push_timestep_vals: F
) -> Result<GdalDataset>
    where
        T: DataSet + VecDataSet,
        P: AsRef<Path>,
        U: GdalType + NumericType,
        F: FnMut(&Option<T>,usize, &mut Vec<U>)
{
    let (nx, ny) = grid_data.dim();
    let dlen = nx * ny;
    if dlen == 0 { return Err( op_failed!("no grid data") ) }

    let utm_zone = utm_rect.utm_zone;
    let center = utm_rect.center();

    let driver = get_gtiff_driver();
    let epsg = utm_zone.epsg();
    let x_min = center.easting - (nx/2) as f64 * x_res;
    let y_max = center.northing + (ny/2) as f64 * y_res;

    let n_steps = time_interval.end - time_interval.start;

    let mut vss: Vec<Vec<U>> = Vec::with_capacity( n_steps);
    for i in  time_interval {
        let mut vs: Vec<U> = Vec::with_capacity( dlen);
        grid_data.iter().map( |c| push_timestep_vals( c, i, &mut vs));
        vss.push( vs);
    }

    Ok( create_grid_ds_from_raster::<U,_>( &driver, path, epsg, x_min, y_max, nx, ny, x_res, y_res, vss, &copts)? )
}

pub const NODATA: f64 = -9999.00;

pub fn wn_utm_grid_to_hrrr<P,T> (region: &str, model_name: &str, utm_grid: &UtmGrid<T>, start: &DateTime<Utc>, n_steps: usize, tgt_dir: P)->Result<Vec<PathBuf>>
    where P: AsRef<Path>, T: WindNinjaDataSet + Clone
{
    let utm_rect = &utm_grid.utm_rect;
    let x_res = utm_grid.x_resolution();
    let y_res = utm_grid.y_resolution();

    let (ny,nx) = utm_grid.dim();
    let dlen = nx*ny;
    let copts = if dlen > 1000 { RasterCreationOptions::from_iter(["COMPRESS=DEFLATE", "PREDICTOR=2"]) } else { RasterCreationOptions::new() };
    let nodata: f32 = -9999.00;
    let n_bands = 4;
    let tstep = utm_grid.time_step_index( start).ok_or( op_failed!("no such time step {}", start))?;

    let mut paths: Vec<PathBuf> = Vec::with_capacity(n_steps);
    for i in 0..n_steps {
        let date = start.clone() + hours(i as u64);
        let fname = odin_data_filename( region, Some(date), &[ model_name ], Some("tif"));
        let path = tgt_dir.as_ref().to_path_buf().join( fname);

        let mut ds = create_utm_ds( &path, &copts, utm_rect, x_res, y_res, tstep + i, nodata, &utm_grid.data, n_bands, push_hrrr_timestep)?;
        set_hrrr_metadata( &mut ds, n_bands, &date.timestamp().to_string())?;

        paths.push( path);
    }

    Ok( paths )
}

/// convert a time step from a UtmGrid with wx forecasts into a single timestep tif with selected/translated bands
/// this does not have to grid the input
pub fn create_utm_ds<T,P,U,F> (
    path: P,
    copts: &RasterCreationOptions,
    utm_rect: &UtmRect,
    x_res: f64,
    y_res: f64,
    tstep: usize,
    nodata_value: U,
    grid_data: &Array2<Option<T>>,
    n_bands: usize,
    mut push_timestep_vals: F
) -> Result<GdalDataset>
    where
        T: DataSet + VecDataSet,
        P: AsRef<Path>,
        U: GdalType + NumericType,
        F: FnMut(&T,usize,&mut Vec<Vec<U>>)
{
    let (nx, ny) = grid_data.dim();
    let dlen = nx * ny;
    if dlen == 0 { return Err( op_failed!("no grid data") ) }

    let utm_zone = utm_rect.utm_zone;
    let center = utm_rect.center();

    let driver = get_gtiff_driver();
    let epsg = utm_zone.epsg();
    let x_min = center.easting - (nx/2) as f64 * x_res;
    let y_max = center.northing + (ny/2) as f64 * y_res;

    let mut vss: Vec<Vec<U>> = vec![ Vec::with_capacity(dlen); n_bands];

    for opt_d in grid_data.iter() {
        if let Some(d) = opt_d.as_ref() {
            push_timestep_vals( d, tstep, &mut vss);
        } else {
            for i in 0..n_bands {
                vss[i].push( nodata_value);
            }
        }
    }

    Ok( create_grid_ds_from_raster::<U,_>( &driver, path, epsg, x_min, y_max, nx, ny, x_res, y_res, vss, &copts)? )
}
