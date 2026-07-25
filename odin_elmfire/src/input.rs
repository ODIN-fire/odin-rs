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

use std::{path::{Path,PathBuf},collections::HashMap, time::Duration};
use odin_wind::errors::op_failed;
use uom::si::{f64::{Angle, Length, Ratio}, velocity::mile_per_hour};
use uom::si::{ratio::{ratio,percent}, length::{meter,kilometer}};
use chrono::{DateTime, Timelike, Utc};
use interp::{interp_slice, InterpMode};
use reqwest::Client;

use odin_common::{
    BoundingBox, datetime::{as_minutes, hours, is_between_inclusive, short_utc_datetime_string}, fs::path_as_str, geo::{GeoPoint,GeoRect}, utm::{UtmPoint,UtmRect,geo_to_utm, try_get_xy_vecs}
};
use odin_fems::{self, FemsConfig, FemsStore, FemsStation, FemsStationKdTree};
use odin_nfdrs::{self, NfdrsConfig, NfdrsRecord, create_nfdrs_inputs, parse_nfdrs_records, run_nfdrs};
use odin_landfire::LANDFIRE_PIXEL_RESOLUTION;
use odin_gdal::{
    CslStringList, errors::OdinGdalError, gdal::raster::RasterCreationOptions, geods::{GeoDs, GeoDsImgType, GeoDsSrs}, get_driver_by_name, get_gtiff_create_opts, get_gtiff_driver, grid::{self, GdalGridAlgorithmOptions, create_grid_ds}
};
use odin_openmeteo::{OpenMeteoConfig, OpenMeteoLocationData, dataset::MinimalSfcData};
use crate::{CACHE_DIR, ElmfireConfig, ElmFire, FixedInputStep, WX_DT, errors::{OdinElmfireError,Result,op_failed}, get_utm_rect_around_point};

fn check_cache<F,T,E> (path: PathBuf, f:F) -> Result<()>
    where F: FnOnce(PathBuf)->std::result::Result<T,E>, OdinElmfireError: From<E>
{
    if path.is_file() {
        //println!("reusing {}", path_as_str(&path)?);
        Ok(())
    } else {
        //println!("creating {}", path_as_str(&path)?);
        f(path)?;
        Ok(())
    }
}

async fn check_cache_async<F,T,E> (path: PathBuf, f:F) -> Result<()>
    where F: AsyncFnOnce(PathBuf)->std::result::Result<T,E>, OdinElmfireError: From<E>
{
    if path.is_file() {
        //println!("reusing {}", path_as_str(&path)?);
        Ok(())
    } else {
        //println!("creating {}", path_as_str(&path)?);
        f(path).await?;
        Ok(())
    }
}

/* #region internal ***********************************************************************************************************/

pub fn create_internal_input (ef: &ElmFire, config: &ElmfireConfig) -> Result<()> {
    let dir = ef.dir.join("inputs");
    let bbox = &ef.utm_rect.bbox;
    let x_width = bbox.east - bbox.west;
    let y_height = bbox.north - bbox.south;
    let dpix = config.lf_res_m as f64;

    let xsize = (x_width / dpix).round() as usize;
    let ysize = (y_height / dpix).round() as usize;

    let driver = get_gtiff_driver();
    let create_opts = get_gtiff_create_opts();

    let adj0 = 1.0; // no initial adjustment
    let phi0 = 1.0; // no initial adjustment

    //--- set surface spread rate adjustment factors
    let path = dir.join("adj.tif");
    if !path.is_file() {
        grid::create_const_grid_ds::<f32,_>(
            &driver, &path, ef.utm_rect.epsg(), bbox.west, bbox.east, bbox.south, bbox.north, xsize, ysize, &[ adj0 ], -9999.0, &create_opts
        )?;
    }

    //--- set initial phi - this is only for new ignitions, otherwise we have to compute its pixel values
    // (for ongoing fires: -1.0 for current fire front but burned area has to be used to modify fbfm40 (e.g. bare land) )
    let path = dir.join("phi.tif");
    if !path.is_file() {
        grid::create_const_grid_ds::<f32,_>(
            &driver, &path, ef.utm_rect.epsg(), bbox.west, bbox.east, bbox.south, bbox.north, xsize, ysize, &[ phi0 ], -9999.0, &create_opts
        )?;
    }

    Ok(())
}

/* #endregion internal */

/* #region fixed steps ****************************************************************************************************/

pub fn create_fixed_input (ef: &ElmFire, config: &ElmfireConfig, steps: &[FixedInputStep]) -> Result<()> {
    if steps.is_empty() {
        return Err( op_failed!("empty fixed input steps"))
    }

    let dir = ef.dir.join("inputs");
    let bbox = &ef.utm_rect.bbox;
    let x_width = bbox.east - bbox.west;
    let y_height = bbox.north - bbox.south;
    let epsg = ef.utm_rect.epsg();

    let driver = get_gtiff_driver();
    let create_opts = get_gtiff_create_opts();
    let n_steps = (ef.n_hours+1) as usize;

    //--- generate the wind inputs

    let dpix = config.wx_res_m as f64;
    let xsize = (x_width / dpix).round() as usize;
    let ysize = (y_height / dpix).round() as usize;

    let band_vals: Vec<f64> = extend( steps.iter().map( |s| s.ws.get::<mile_per_hour>()).collect(), n_steps);
    grid::create_const_grid_ds::<f32,_>(
        &driver, &dir.join("ws.tif"), epsg, bbox.west, bbox.east, bbox.south, bbox.north, xsize, ysize, &band_vals, -9999.0, &create_opts
    )?;

    let band_vals: Vec<f64> = extend( steps.iter().map( |s| s.wd.degrees()).collect(), n_steps);
    grid::create_const_grid_ds::<f32,_>(
        &driver, &dir.join("wd.tif"), epsg, bbox.west, bbox.east, bbox.south, bbox.north, xsize, ysize, &band_vals, -9999.0, &create_opts
    )?;

    //--- generate the fuel moisture inputs

    let dpix = config.nfdrs_res_m as f64;
    let xsize = (x_width / dpix).round() as usize;
    let ysize = (y_height / dpix).round() as usize;

    let band_vals: Vec<f64> = extend( steps.iter().map( |s| s.m1.get::<percent>()).collect(), n_steps);
    grid::create_const_grid_ds::<f32,_>(
        &driver, &dir.join("m1.tif"), epsg, bbox.west, bbox.east, bbox.south, bbox.north, xsize, ysize, &band_vals, -9999.0, &create_opts
    )?;

    let band_vals: Vec<f64> = extend( steps.iter().map( |s| s.m10.get::<percent>()).collect(), n_steps);
    grid::create_const_grid_ds::<f32,_>(
        &driver, &dir.join("m10.tif"), epsg, bbox.west, bbox.east, bbox.south, bbox.north, xsize, ysize, &band_vals, -9999.0, &create_opts
    )?;

    let band_vals: Vec<f64> = extend( steps.iter().map( |s| s.m100.get::<percent>()).collect(), n_steps);
    grid::create_const_grid_ds::<f32,_>(
        &driver, &dir.join("m100.tif"), epsg, bbox.west, bbox.east, bbox.south, bbox.north, xsize, ysize, &band_vals, -9999.0, &create_opts
    )?;

    let band_vals: Vec<f64> = extend( steps.iter().map( |s| s.lh.get::<percent>()).collect(), n_steps);
    grid::create_const_grid_ds::<f32,_>(
        &driver, &dir.join("lh.tif"), epsg, bbox.west, bbox.east, bbox.south, bbox.north, xsize, ysize, &band_vals, -9999.0, &create_opts
    )?;

    let band_vals: Vec<f64> = extend( steps.iter().map( |s| s.lw.get::<percent>()).collect(), n_steps);
    grid::create_const_grid_ds::<f32,_>(
        &driver, &dir.join("lw.tif"), epsg, bbox.west, bbox.east, bbox.south, bbox.north, xsize, ysize, &band_vals, -9999.0, &create_opts
    )?;

    Ok(())
}

fn extend<T: Clone> (mut vs: Vec<T>, n: usize)->Vec<T> {
    let len = vs.len();
    if len <= n {
        for i in len..n {
            vs.push( vs[len-1].clone())
        }
    }
    vs
}

/* #endregion  fixed steps */

/* #region NFDRS (fuel moistures) *****************************************************************************************/

// although we could get the fuel moisture input from FEMS it is advisible to compute the values based on the grid
// we are simulating. Using FEMS would not work if there are no stations in range

pub async fn create_nfdrs_input (ef: &ElmFire, config: &ElmfireConfig)->Result<()> {
    let wx_config: OpenMeteoConfig = odin_openmeteo::load_config( &config.wx_config_name)?;
    let lf_src: GeoDs = odin_landfire::load_config( &config.lf_src_name)?;
    let nfdrs_config: NfdrsConfig = odin_nfdrs::load_config( &config.nfdrs_config_name)?;
    let nodata: f64 = -9999.0;

    let dir = ef.dir.join("inputs");
    let forecast_days = ef.wx_forecast_days();
    let past_days = config.nfdrs_past_days; // we need some to predict 10/100 h DFM based on last weeks' weather
    let wx_model = &config.wx_model;

    let start_date = ef.base_date.clone();
    let end_date = start_date + hours( ef.n_hours as u64);
    let n_time_steps = ef.n_hours as usize + 1;

    let center = ef.utm_rect.center();
    let bbox_size = Length::new::<meter>( config.bbox_size_m as f64);
    let raster = Length::new::<meter>( config.fm_raster_m as f64);
    let grid_points = center.grid_points_from_center( bbox_size, bbox_size, raster, false);
    let n_grid_points = grid_points.len();
    let (mut xs,mut ys) = try_get_xy_vecs( &grid_points, ef.utm_rect.utm_zone)?;

    // the band data vecs we need for respective tif data sets
    let mut m1 = vec![ Vec::<f64>::with_capacity(n_grid_points); n_time_steps];
    let mut m10 = vec![ Vec::<f64>::with_capacity(n_grid_points); n_time_steps];
    let mut m100 = vec![ Vec::<f64>::with_capacity(n_grid_points); n_time_steps];
    let mut lh = vec![ Vec::<f64>::with_capacity(n_grid_points); n_time_steps];
    let mut lw = vec![ Vec::<f64>::with_capacity(n_grid_points); n_time_steps];

    let mut dropped = 0;

    for (i,p) in grid_points.iter().enumerate() {
        let pos: GeoPoint = p.into();
        let output_path = odin_nfdrs::output_path( &pos, &ef.base_date, forecast_days, past_days, wx_model);

        match create_nfdrs_inputs( &wx_config, &lf_src, &pos, &ef.base_date, forecast_days, past_days, wx_model, &output_path).await {
            Ok(cfg_path) => {
                run_nfdrs( &nfdrs_config.nfdrs_cmd, &cfg_path).await?;
                let recs: Vec<NfdrsRecord> = parse_nfdrs_records( &output_path, |rec| is_between_inclusive( &rec.date, &start_date, &end_date))?;

                for (j,r) in recs.iter().enumerate() {
                    m1[j].push( r.dfm_1h.get::<percent>());
                    m10[j].push( r.dfm_10h.get::<percent>());
                    m100[j].push( r.dfm_100h.get::<percent>());
                    lh[j].push( r.lfm_herb.get::<percent>());
                    lw[j].push( r.lfm_woody.get::<percent>());
                }
            }
            Err(e) => { // remove this grid point as it does not contribute any data - might be water or developed
                println!("skipping grid point {} : {}", pos, e);
                xs.remove(i - dropped);
                ys.remove(i - dropped);
                dropped += 1;
            }
        }
    }

    if xs.is_empty() {
        return Err( op_failed!("no valid grid data"));
    }

    //println!("m1:  {:?}", m1[0]);
    //println!("m10: {:?}", m10[0]);
    //println!("lw:  {:?}", lw[0]);

    //--- the invariants for all fuel moisture data sets
    let bbox = &ef.utm_rect.bbox;
    let x_width = bbox.width();
    let y_height = bbox.height();
    let dpix = config.nfdrs_res_m as f64; /// pixel resolution in m  ?? use same res as wind (multiple of lf_res)

    let xsize = (x_width / dpix).round() as usize;
    let ysize = (y_height / dpix).round() as usize;

    let drv = get_gtiff_driver();
    let epsg = ef.utm_rect.epsg();
    let (w,s,e,n) = (bbox.west, bbox.south, bbox.east, bbox.north);
    let alg = GdalGridAlgorithmOptions::inv_dist( 2.0, config.fm_raster_m as f64 * 0.5, bbox.width(), -9999.0);
    //let alg = GdalGridAlgorithmOptions::nearest_neighbor_within( config.bbox_size_m as f64, -9999.0);
    let copts = get_gtiff_create_opts();

    check_cache( dir.join("m1.tif"), |path|
        create_grid_ds::<f32,_>(&drv, path, epsg, w, e, s, n, xsize, ysize, &xs, &ys, m1.as_slice(), &alg, &copts)
    )?;

    check_cache( dir.join("m10.tif"), |path|
        create_grid_ds::<f32,_>(&drv, &path, epsg, w, e, s, n, xsize, ysize, &xs, &ys, m10.as_slice(), &alg, &copts)
    )?;

    check_cache( dir.join("m100.tif"), |path|
        create_grid_ds::<f32,_>(&drv, &path, epsg, w, e, s, n, xsize, ysize, &xs, &ys, m100.as_slice(), &alg, &copts)
    )?;

    check_cache( dir.join("lh.tif"), |path|
        create_grid_ds::<f32,_>(&drv, &path, epsg, w, e, s, n, xsize, ysize, &xs, &ys, lh.as_slice(), &alg, &copts)
    )?;

    check_cache( dir.join("lw.tif"), |path|
        create_grid_ds::<f32,_>(&drv, &path, epsg, w, e, s, n, xsize, ysize, &xs, &ys, lw.as_slice(), &alg, &copts)
    )?;

    Ok(())
}

/* #endregion NFDRS */

/* #region FEMS ***********************************************************************************************************/

// see NFDRS comments (which we prefer for fuel moisture data)

pub async fn create_fems_input (ef: &ElmFire, config: &ElmfireConfig) -> Result<()> {
    let fems_config: FemsConfig = odin_fems::load_config( &config.fems_config_name)?;
    let bbox_size = Length::new::<meter>( ef.utm_rect.width()); // we use that as edge length for sim bbox
    let station_dist = bbox_size * 1.77; // max distance for FEMS station lookup (2/sqrt(2) * 1.5)
    let geo_center: GeoPoint = ef.utm_rect.center().into();

    let kd_tree = FemsStationKdTree::load_from( &fems_config.station_metadata)?;
    //println!("{} stations loaded", kd_tree.len());

    let stations = kd_tree.station_distances_within( &geo_center, station_dist);
    if stations.is_empty() {
        return Err( op_failed!("no stations found within radius of {} km", station_dist.get::<kilometer>()) )
    }

    // println!("{} stations found within {} km of {}:", stations.len(), station_dist.get::<kilometer>(), ignition_pos);
    // for (s,d) in stations.iter() {
    //     let s = &s.metadata;
    //     let pos = &s.position;
    //     println!("    {:8} {:20} [{:.4},{:.4}] : {:.1}km", s.id, s.name, pos.longitude_degrees(), pos.latitude_degrees(), d.get::<kilometer>() );
    // }

    let mut stations = FemsStore::new( stations.into_iter().map( |(s,d)| s).collect());
    let client = Client::new();

    stations.retrieve_current_obs( &client, &fems_config.url, &ef.name, ef.n_hours).await?;

    let fems_pixel_res = Length::new::<meter>( config.fems_res_m as f64);
    let stations: Vec<&FemsStation> = stations.values().collect();

    let dir = ef.dir.join("inputs");
    let wx_timestamps = ef.get_wx_timestamps();
    create_fems_grids( &stations, &ef.utm_rect, fems_pixel_res, &wx_timestamps, &dir)?;

    Ok(())
}

pub fn create_fems_grids (
    stations: &[&FemsStation],
    utm_rect: &UtmRect,
    pixel_res: Length,
    wx_timestamps: &[i64],
    dir: &PathBuf
) -> Result<()>
{
    let wx_times: Vec<f64> = wx_timestamps.iter().map(|ts| *ts as f64).collect();
    let time_steps = wx_times.len();
    let n_stations = stations.len();

    let utm_points: Vec<UtmPoint> = stations.iter().map( |s| {
        let p = s.position.as_geo_point();
        geo_to_utm(&p).ok_or( op_failed!("invalid station position"))
    }).collect::<Result<_>>()?;
    let (xs,ys) = try_get_xy_vecs( &utm_points, utm_rect.utm_zone)?;

    let create_opts = get_gtiff_create_opts();

    check_cache( dir.join("m1.tif"), |path| {
        create_fems_grid_ds( &path, &create_opts, &utm_rect, pixel_res, &wx_times, stations, &xs, &ys,
            |s| s.get_dfm_1h_values().iter().map( |v| v.get::<percent>() as f64).collect())
    })?;

    check_cache( dir.join("m10.tif"), |path| {
        create_fems_grid_ds( &path, &create_opts, &utm_rect, pixel_res, &wx_times, stations, &xs, &ys,
            |s| s.get_dfm_10h_values().iter().map( |v| v.get::<percent>() as f64).collect())
    })?;

    check_cache( dir.join("m100.tif"), |path| {
        create_fems_grid_ds( &path, &create_opts, &utm_rect, pixel_res, &wx_times, stations, &xs, &ys,
            |s| s.get_dfm_100h_values().iter().map( |v| v.get::<percent>() as f64).collect())
    })?;

    check_cache( dir.join("lh.tif"), |path| {
        create_fems_grid_ds( &path, &create_opts, &utm_rect, pixel_res, &wx_times, stations, &xs, &ys,
            |s| s.get_lfm_herb_values().iter().map( |v| v.get::<percent>() as f64).collect())
    })?;

    check_cache( dir.join("lw.tif"), |path| {
        create_fems_grid_ds( &path, &create_opts, &utm_rect, pixel_res, &wx_times, stations, &xs, &ys,
            |s| s.get_lfm_wood_values().iter().map( |v| v.get::<percent>() as f64).collect())
    })?;

    Ok(())
}

/// per data type (e.g. 1h dead fuel moisture) function to create a grid from respective time series of all stations
/// note that Elmfire expects equidistant time steps between the bands of this grid and the first band might have to be
/// extrapolated if the leading observation time (which is the last reporting time of the station) is after the initial
/// time step (15min raster) we request. All following forecast values for the station will be at full hours, i.e. for
/// dt < 60min intervals we have to interpolate values. Since FEMS variables (mostly dead fuel moisture) don't change
/// drastically we just use linear interpolation (which also makes extrapolation easier).
/// e.g. [ 8:22, 9:00, 10:00, .. ] -> [ 8:15, 8:30, 8:45, 9:00, 9:15, 9:30, ..] for a station that reports at h:22
fn create_fems_grid_ds <P,F> (
    path: P,
    create_opts: &RasterCreationOptions,
    utm_rect: &UtmRect,
    res: Length,
    sim_times: &[f64],
    stations: &[&FemsStation],
    xs: &[f64], // station position easting values
    ys: &[f64], // station position northing values
    get_values: F // data value retriever (e.g. for 1h dead fuel moisture)
)->Result<()>
    where P: AsRef<Path>, F: Fn(&FemsStation)->Vec<f64> // caller profided function to retrieve a specific FemsStation obs
{
    let time_steps = sim_times.len();
    let n_stations = stations.len();

    // get interpolated values for each station for this data type
    let mut station_values: Vec<Vec<f64>> = Vec::with_capacity( n_stations);
    for station in stations.iter() {
        let times_obs: Vec<f64> = station.get_nfdrs_obs_dates().iter().map( |d| d.timestamp() as f64).collect();
        let vs_obs: Vec<f64> = get_values(station);
        let vs_interp: Vec<f64> = interp_slice( &times_obs, &vs_obs, sim_times, &InterpMode::Extrapolate);
        station_values.push( vs_interp);
    }

    // and turn this into an array of timestep value arrays (containing all stations)
    let mut ts_values: Vec<Vec<f64>> = Vec::with_capacity(time_steps);
    for i in 0..time_steps {
        let mut vs: Vec<f64> = Vec::with_capacity( n_stations);
        for j in 0..n_stations {
            vs.push( station_values[j][i])
        }
        ts_values.push( vs);
    }

    let bbox = &utm_rect.bbox;
    let x_width = bbox.width();
    let y_height = bbox.height();
    let dpix = res.get::<meter>();
    let alg = GdalGridAlgorithmOptions::linear( 50000.0, -9999.0); // use any station in 50km radius (this is the crux!)

    let xsize = (x_width / dpix).round() as usize;
    let ysize = (y_height / dpix).round() as usize;

    let driver = get_gtiff_driver();
    let ds = grid::create_grid_ds::<f32,_>(
        &driver,
        &path,
        utm_rect.epsg(),
        bbox.west, bbox.east,
        bbox.south, bbox.north,
        xsize, ysize,
        &xs, &ys, &ts_values,
        &alg,
        create_opts
    )?;

    //let filename = path.as_ref().file_name().unwrap_or(std::ffi::OsStr::new("?"));
    //println!("created {}x{} grid {:?} with {} time steps", xsize, ysize, filename, time_steps);

    Ok(())
}

/* #endregion FEMS */

/* #region landfire ***********************************************************************************************************/

pub async fn create_landfire_input (ef: &ElmFire, config: &ElmfireConfig)->Result<()> {
    let lf_src: GeoDs = odin_landfire::load_config( &config.lf_src_name)?;

    let srs = GeoDsSrs::from_epsg( ef.utm_rect.epsg()).ok_or( op_failed!("invalid UTM coordinates") )?;
    let lf_pixel_res = Length::new::<meter>( config.lf_res_m as f64);
    let img_type = odin_gdal::geods::GeoDsImgType::TIF;

    let dir = ef.dir.join("inputs");
    create_landfire_grids( &lf_src, &ef.utm_rect, lf_pixel_res, &dir).await?;

    Ok(())
}

pub async fn create_landfire_grids (lf_src: &GeoDs, utm_rect: &UtmRect, pixel_res: Length, dir: &PathBuf) -> Result<()> {
    let srs = GeoDsSrs::from_epsg( utm_rect.epsg()).ok_or( op_failed!("invalid UTM bbox coordinates") )?;
    let img_type = odin_gdal::geods::GeoDsImgType::TIF;
    let res = pixel_res.get::<meter>();

    // NOTE - these are i16 grids - Elmfire chokes if nodata is not a rounded value

    check_cache_async( dir.join("fbfm40.tif"), async |path|
        lf_src.get_res_file( "FBFM40", &utm_rect.bbox, srs, res, res, Some(-9999.0), img_type, &path).await
    ).await?;

    check_cache_async( dir.join("cbd.tif"), async |path|
        lf_src.get_res_file( "CBD", &utm_rect.bbox, srs, res, res, Some(-9999.0), img_type, &path).await
    ).await?;

    check_cache_async( dir.join("cbh.tif"), async |path|
        lf_src.get_res_file( "CBH", &utm_rect.bbox, srs, res, res, Some(-9999.0), img_type, &path).await
    ).await?;

    check_cache_async( dir.join("cc.tif"), async |path|
        lf_src.get_res_file( "CC", &utm_rect.bbox, srs, res, res, Some(-9999.0), img_type, &path).await
    ).await?;

    check_cache_async( dir.join("ch.tif"), async |path|
        lf_src.get_res_file( "CH", &utm_rect.bbox, srs, res, res, Some(-9999.0), img_type, &path).await
    ).await?;

    check_cache_async( dir.join("slp.tif"), async |path|
        lf_src.get_res_file( "SlpD", &utm_rect.bbox, srs, res, res, Some(-9999.0), img_type, &path).await
    ).await?;

    check_cache_async( dir.join("asp.tif"), async |path|
        lf_src.get_res_file( "Asp", &utm_rect.bbox, srs, res, res, Some(-9999.0), img_type, &path).await
    ).await?;

    check_cache_async( dir.join("dem.tif"), async |path|
        lf_src.get_res_file( "Elev", &utm_rect.bbox, srs, res, res, Some(-9999.0), img_type, &path).await
    ).await?;

    Ok(())
}

/* #endregion landfire */


/* #region wx ***********************************************************************************************************/

pub async fn create_wx_input (ef: &ElmFire, config: &ElmfireConfig)->Result<()> {
    let geo_rect = ef.utm_rect.to_geo_rect(odin_common::utm::GeoRectMapping::ExteriorGeo);
    let wx_config: OpenMeteoConfig = odin_openmeteo::load_config( &config.wx_config_name)?;
    let pix_res = config.wx_res_m as f64;
    let raster_m = config.wind_raster_m as f64;
    let days = ef.wx_forecast_days();

    let vs: Vec<OpenMeteoLocationData<MinimalSfcData>> =
        OpenMeteoLocationData::get_hourly_bbox_forecasts( &wx_config, &geo_rect, days, 0, &config.wx_model).await?;

    let dir = ef.dir.join("inputs");
    create_wx_grids( &vs, &ef.utm_rect, raster_m, pix_res, ef.n_hours, &dir)?;

    Ok(())
}

// note that OpenMeteo grid points are geo (and Gaussian)
// note also that vs has to be properly aligned according to WX_DT (15min)
fn create_wx_grids (
    vs: &[OpenMeteoLocationData<MinimalSfcData>],
    utm_rect: &UtmRect,
    raster_m: f64,
    pix_res_m: f64,
    n_hours: u32,
    dir: &PathBuf
) -> Result<()>
{
    let utm_center = utm_rect.center();
    let geo_bbox = utm_rect.to_geo_rect( odin_common::utm::GeoRectMapping::ExteriorGeo);
    let utm_zone = utm_rect.utm_zone;
    let gps = OpenMeteoLocationData::geo_points(&vs);
    let utm_pos: Vec<UtmPoint> = UtmPoint::from_geo_points_in_zone( &gps, utm_zone)?;
    let (xs,ys) = try_get_xy_vecs( &utm_pos, utm_zone)?; // x,y UTM locations for each grid point
    let n_steps = n_hours as usize + 1;

    if vs.is_empty() { return Err( op_failed!("no wx data") ) }
    let vs_len = vs.len();

    // each row holds values of all grid points for this time step
    let mut ws_data: Vec<Vec<f64>> = Vec::with_capacity(n_steps);
    let mut wd_data: Vec<Vec<f64>> = Vec::with_capacity(n_steps);

    for i in 0..n_steps { // save values for each requested time step
        let mut ws: Vec<f64> = Vec::with_capacity( vs_len); // wind speed (m/s)
        let mut wd: Vec<f64> = Vec::with_capacity( vs_len); // wind direction (degrees)

        for gp in vs.iter() {
            let hd = gp.hourly_data.as_ref().ok_or( op_failed!("no hourly data for observation"))?;

            // Elmfire expects wind speed in mph, openmeteo reports it as m/s
            ws.push( hd.hourly.wind_speed_10m[i] as f64 * 2.23694);
            wd.push( hd.hourly.wind_direction_10m[i] as f64);
        }

        ws_data.push(ws);
        wd_data.push(wd);
    }

    let driver = get_gtiff_driver();
    let epsg = utm_zone.epsg();
    let x_min = utm_rect.west;
    let x_max = utm_rect.east;
    let y_min = utm_rect.south;
    let y_max = utm_rect.north;
    let x_size = (utm_rect.width() / pix_res_m) as usize;
    let y_size = (utm_rect.height() / pix_res_m) as usize;
    let alg = GdalGridAlgorithmOptions::inv_dist( 2.0, raster_m * 0.5, utm_rect.width(), -9999.0);
    //let alg = GdalGridAlgorithmOptions::nearest_neighbor_within( utm_rect.width(), -9999.0);
    //let alg = GdalGridAlgorithmOptions::linear( raster_m, -9999.0);
    let copts = get_gtiff_create_opts();

    check_cache( dir.join("wd.tif"), |path| {
        create_grid_ds::<f32,_>( &driver, &path, epsg, x_min, x_max, y_min, y_max, x_size, y_size, &xs, &ys, &wd_data, &alg, &copts)
    })?;

    check_cache( dir.join("ws.tif"), |path| {
        create_grid_ds::<f32,_>( &driver, &path, epsg, x_min, x_max, y_min, y_max, x_size, y_size, &xs, &ys, &ws_data, &alg, &copts)
    })?;

    Ok(())
}
