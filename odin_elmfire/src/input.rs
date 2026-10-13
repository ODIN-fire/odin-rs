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

use std::{path::{Path,PathBuf},collections::HashMap, time::Duration, str::FromStr, sync::Arc};
use tokio::time::sleep;
use uom::si::f64::{Angle, Length, Ratio};
use uom::si::{ratio::{ratio,percent}, length::{meter,kilometer,inch}, angle::{degree}, velocity::{meter_per_second,mile_per_hour}, thermodynamic_temperature::degree_celsius};
use chrono::{DateTime, Timelike, Utc};
use interp::{interp_slice, InterpMode};
use reqwest::Client;
use ndarray::Array2;
use gdal::{Dataset as GdalDataset};

use odin_common::{
    BoundingBox, arc,
    collections::Grid,
    datetime::{as_minutes, days, hours, is_between_inclusive, secs, short_utc_datetime_string, ymd, ymd_hms_enc},
    fs::{create_symlink, ensure_writable_dir, odin_data_filename, path_as_str, path_to_lossy_string, replace_env_var_path},
    geo::{GeoPoint,GeoRect},
    uom::{LengthF32, ToF64Quantity,VelocityF32},
    utm::{UtmPoint,UtmRect,geo_to_utm, try_get_xy_vecs}
};
use odin_fems::{self, FemsConfig, FemsStore, FemsStation, FemsStationKdTree};
use odin_nfdrs::{self, NfdrsConfig, NfdrsLocationClassification, NfdrsRecord, compute_nfdrs_recs, create_nfdrs_inputs, output_path, parse_nfdrs_records, run_nfdrs};
use odin_landfire::LANDFIRE_PIXEL_RESOLUTION;
use odin_gdal::{
    CslStringList, GdalDataType, copy_proj, create_dataset, find_rasterband_index,
    get_driver_by_name, get_gtiff_create_opts, get_gtiff_driver, get_rasterband_as_vec,
    open_dataset, read_rasterband_into_slice, set_rasterband_from_vec, get_raster_info,
    gdal::raster::RasterCreationOptions,
    errors::OdinGdalError,
    geods::{GeoDs, GeoDsImgType, GeoDsSrs},
    grid::{self, GdalGridAlgorithmOptions, create_grid_ds_from_array2,create_grid_ds_from_raster, create_grid_ds_from_scatter},
};
use odin_wind::{WindConfig, WnJob, get_wn_out_basename, run_wn, wn_output_path};
use odin_openmeteo::{
    HourlyLocationData, MAX_POINTS, OpenMeteoConfig,
    convert::{create_stacked_hourly_utm_ds, set_hrrr_metadata, wn_utm_grid_to_hrrr},
    dataset::{Fw21Data, MinimalSfcData}, get_forecast_url, get_time_step_range,
    grid::{GridAlgorithm, UtmGrid}, ids::ModelId, queries::get_cached_annual_grid_precipitations
};
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

pub async fn create_fixed_input (ef: &ElmFire, config: &ElmfireConfig, steps: &[FixedInputStep]) -> Result<()> {
    if steps.is_empty() {
        return Err( op_failed!("empty fixed input steps"))
    }

    let dir = ef.dir.join("inputs");

    create_fixed_wind_input( ef, config, steps, &dir).await?;
    create_fixed_fuel_moisture_input( ef, config, steps, &dir)?;

    Ok(())
}

/// create the ws, wd tifs either directly from the fixed input steps or from WindNinja output
async fn create_fixed_wind_input (ef: &ElmFire, config: &ElmfireConfig, steps: &[FixedInputStep], dir: &PathBuf) -> Result<()> {
    if ef.compute_wind { // use FixedInputSteps to create WindNinja input and use WindNinja output as Elmfire input
        let wind_dir = dir.join("wn");
        ensure_writable_dir( &wind_dir)?;

        let wx_paths = create_fixed_wn_input_steps( ef, config, steps, &wind_dir)?;
        run_wn_steps( ef, config, &wx_paths, &wind_dir, dir).await?

    } else {
        let bbox = &ef.utm_rect.bbox;
        let x_width = bbox.east - bbox.west;
        let y_height = bbox.north - bbox.south;
        let epsg = ef.utm_rect.epsg();
        let n_steps = (ef.n_hours+1) as usize;

        let driver = get_gtiff_driver(); // not Send - has to be obtained after await points
        let create_opts = get_gtiff_create_opts();

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
    }

    Ok(())
}

/// create the m1, m10, m100, lh, lw tifs from the fixed input steps
fn create_fixed_fuel_moisture_input (ef: &ElmFire, config: &ElmfireConfig, steps: &[FixedInputStep], dir: &PathBuf) -> Result<()> {
    let bbox = &ef.utm_rect.bbox;
    let x_width = bbox.east - bbox.west;
    let y_height = bbox.north - bbox.south;
    let epsg = ef.utm_rect.epsg();
    let n_steps = (ef.n_hours+1) as usize;

    let driver = get_gtiff_driver();
    let create_opts = get_gtiff_create_opts();

    let dpix = config.wx_res_m as f64;
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

/// create HRRR conform WindNinja wx input tifs (one per hour) with const band values from the fixed input steps
/// note this is not async since the driver is not Send. Datasets are dropped (written) before we return
fn create_fixed_wn_input_steps (ef: &ElmFire, config: &ElmfireConfig, steps: &[FixedInputStep], wind_dir: &PathBuf)->Result<Vec<PathBuf>> {
    let n_steps = (ef.n_hours+1) as usize;
    let steps = extend( steps.to_vec(), n_steps); // repeat last step if we have fewer than n_steps

    let driver = get_gtiff_driver();
    let create_opts = get_gtiff_create_opts();
    let epsg = ef.utm_rect.epsg();

    // pad the bbox by one wx grid cell since WindNinja rejects wx input that does not fully cover the DEM
    let dpix = config.wx_res_m as f64;
    let bbox = &ef.utm_rect.bbox;
    let (w,s,e,n) = (bbox.west - dpix, bbox.south - dpix, bbox.east + dpix, bbox.north + dpix);
    let xsize = ((e - w) / dpix).round() as usize;
    let ysize = ((n - s) / dpix).round() as usize;

    let mut paths: Vec<PathBuf> = Vec::with_capacity(n_steps);
    for (i,step) in steps.iter().take(n_steps).enumerate() {
        let date = ef.base_date + hours(i as u64);
        let path = wind_dir.join( odin_data_filename( &ef.name, Some(date), &[ &config.wx_model ], Some("tif")));

        // HRRR band order and units: TMP [C], TCDC [%], UGRD [m/s], VGRD [m/s]
        // wd is the meteorological direction the wind is coming from
        let spd = step.ws.get::<meter_per_second>();
        let u = -spd * step.wd.sin();
        let v = -spd * step.wd.cos();
        let band_vals = [ step.t2.get::<degree_celsius>(), step.tcc.get::<percent>(), u, v ];

        let mut ds = grid::create_const_grid_ds::<f32,_>(
            &driver, &path, epsg, w, e, s, n, xsize, ysize, &band_vals, -9999.0, &create_opts
        )?;
        set_hrrr_metadata( &mut ds, band_vals.len(), &date.timestamp().to_string())?;

        paths.push( path);
    }

    Ok(paths)
}


/* #endregion  fixed steps */

/* #region actual weather inputs (combined NFDRS and wind inputs) *********************************************************/

pub async fn create_wx_based_input (ef: &ElmFire, config: &ElmfireConfig)->Result<()> {
    let wx_config: OpenMeteoConfig = odin_openmeteo::load_config( &config.wx_config_name)?;
    let lf_src: GeoDs = odin_landfire::load_config( &config.lf_src_name)?;
    let nfdrs_config: NfdrsConfig = odin_nfdrs::load_config( &config.nfdrs_config_name)?;
    let nodata: f64 = -9999.0;

    let dir = ef.dir.join("inputs");
    let past_days = config.nfdrs_past_days; // we need some to predict 10/100 h DFM based on last weeks' weather
    let wx_model = ModelId::from_str( &config.wx_model)?;
    let wx_res = config.wx_res_m as f64;

    let start_date = ef.base_date - days( past_days as u64);
    let end_date = ef.base_date + hours( ef.n_hours as u64);
    let n_time_steps = ef.n_hours as usize + 1; // this is forecast-only (number of rasterbands for Elmfire input)

    let n2 = ((ef.utm_rect.width() / wx_res).ceil() as usize) / 2;  // number of grid points in each direction from center
    if (2*n2 + 1).pow(2) >= MAX_POINTS {
        return Err( op_failed!("too many grid points"))
    }

    // the cached weather data acquisition
    let fw21_grid = get_fw21_grid( &wx_config, wx_model, &dir, &ef.geo_center, Length::new::<meter>(wx_res), n2, &start_date, &end_date).await?;
    let precip_grid = get_annual_precip( &wx_config, &dir, &fw21_grid, &ef.geo_center, &ef.base_date).await?;

    let (ny,nx) = fw21_grid.dim();
    let n_grid_points = nx * ny;
    let nodata: f32 = -9999.0;

    // the band data vecs we need for respective tif data sets (forecast only)
    let mut m1:   Vec<Array2<f32>> = vec![ Array2::from_elem( (ny,nx), nodata); n_time_steps];
    let mut m10:  Vec<Array2<f32>> = vec![ Array2::from_elem( (ny,nx), nodata); n_time_steps];
    let mut m100: Vec<Array2<f32>> = vec![ Array2::from_elem( (ny,nx), nodata); n_time_steps];
    let mut lh:   Vec<Array2<f32>> = vec![ Array2::from_elem( (ny,nx), nodata); n_time_steps];
    let mut lw:   Vec<Array2<f32>> = vec![ Array2::from_elem( (ny,nx), nodata); n_time_steps];
    let mut ws:   Vec<Array2<f32>> = vec![ Array2::from_elem( (ny,nx), nodata); n_time_steps];
    let mut wd:   Vec<Array2<f32>> = vec![ Array2::from_elem( (ny,nx), nodata); n_time_steps];

    for j in 0..ny {
        for i in 0..nx {
            let grid_point = (j,i); // ndarray uses row major = (row,col)
            let pos = fw21_grid.geo_pos_at( i, j)?; // we use (col,row)

            let loc_cls = NfdrsLocationClassification::for_location( &lf_src, &pos).await?;
            let annual_precip = precip_grid.at( i, j); // note this is (col,row)

            if let Some(fw21) = fw21_grid.at( i, j)? {  // note this is (col,row)
                let nfdrs_recs = compute_nfdrs_recs( &nfdrs_config, &pos, fw21, annual_precip, &loc_cls, wx_model, &ef.base_date).await?;

                for (k,rec) in nfdrs_recs.iter().enumerate() {
                    m1[k][grid_point] = rec.dfm_1h.get::<percent>() as f32;
                    m10[k][grid_point] = rec.dfm_10h.get::<percent>() as f32;
                    m100[k][grid_point] = rec.dfm_100h.get::<percent>() as f32;
                    lh[k][grid_point] = rec.lfm_herb.get::<percent>() as f32;
                    lw[k][grid_point] = rec.lfm_woody.get::<percent>() as f32;

                    ws[k][grid_point] = rec.wind_speed.get::<mile_per_hour>() as f32;
                    wd[k][grid_point] = rec.wind_azimuth.get::<degree>() as f32;
                }
            }
        }
    }

    // if we compute wind the wx-based ws/wd are only kept for reference - ws.tif and wd.tif are created from WindNinja output
    let (ws_fname, wd_fname) = if ef.compute_wind { ("ws_wx.tif", "wd_wx.tif") } else { ("ws.tif", "wd.tif") };

    { // we need to drop drv before any further await point since it is not Send
        let drv = get_gtiff_driver();
        let epsg = fw21_grid.utm_rect.epsg();
        let copts = get_gtiff_create_opts();
        let bbox = &fw21_grid.utm_rect;
        let (w,s,e,n) = (bbox.west, bbox.south, bbox.east, bbox.north);

        check_cache( dir.join("m1.tif"),   |path| create_grid_ds_from_array2::<f32,_>(&drv, path, epsg, w, n, nx, ny, wx_res, wx_res, m1, &copts))?;
        check_cache( dir.join("m10.tif"),  |path| create_grid_ds_from_array2::<f32,_>(&drv, path, epsg, w, n, nx, ny, wx_res, wx_res, m10, &copts))?;
        check_cache( dir.join("m100.tif"), |path| create_grid_ds_from_array2::<f32,_>(&drv, path, epsg, w, n, nx, ny, wx_res, wx_res, m100, &copts))?;
        check_cache( dir.join("lh.tif"),   |path| create_grid_ds_from_array2::<f32,_>(&drv, path, epsg, w, n, nx, ny, wx_res, wx_res, lh, &copts))?;
        check_cache( dir.join("lw.tif"),   |path| create_grid_ds_from_array2::<f32,_>(&drv, path, epsg, w, n, nx, ny, wx_res, wx_res, lw, &copts))?;
        check_cache( dir.join(ws_fname),   |path| create_grid_ds_from_array2::<f32,_>(&drv, path, epsg, w, n, nx, ny, wx_res, wx_res, ws, &copts))?;
        check_cache( dir.join(wd_fname),   |path| create_grid_ds_from_array2::<f32,_>(&drv, path, epsg, w, n, nx, ny, wx_res, wx_res, wd, &copts))?;
    }

    if ef.compute_wind {
        compute_wind( ef, config, &fw21_grid, &dir).await?;
    }

    Ok(())
}

async fn get_fw21_grid (
    wx_config: &OpenMeteoConfig, wx_model: ModelId, dir: &PathBuf,
    geo_center: &GeoPoint, res: Length, n2: usize,
    start: &DateTime<Utc>, end: &DateTime<Utc>
) -> Result<UtmGrid<Fw21Data>>
{
    let mut grid: UtmGrid<Fw21Data> = UtmGrid::from_center( geo_center, res, n2, n2)?;

    // make sure we get all the OpenMeteo data in one query
    let center_enc = geo_center.encode(4);
    let path = dir.join( format!("fw21__{}__{}.json", center_enc, ymd_hms_enc( end)));
    grid.get_cached_hourly_data( wx_config, wx_model, start, end, GridAlgorithm::idw( 1.5, 2.0), &path).await?;

    Ok(grid)
}

// NOTE - using odin_common::containers::Grid is a workaround for the infamous
//       "Future not Send.. expected struct `Quantity<..., ..., f64>` found struct `Quantity<..., ..., f64>`"
// With ndarray v0.17.2 we can't directly use Array2<Length> in the loop and UtmGrid should not have any DeserializeOwned
// bounds on the type (only on the deserializing methods)
// TODO - remove workaround if fixed in rustc (https://github.com/rust-lang/rust/issues/162558)
// and/or ndarray (https://github.com/rust-ndarray/ndarray/issues/1617)

async fn get_annual_precip (
    wx_config: &OpenMeteoConfig, dir: &PathBuf,
    grid: &UtmGrid<Fw21Data>, geo_center: &GeoPoint,
    date: &DateTime<Utc>
) -> Result<Grid<Length>>
{
    let center_enc = geo_center.encode(4);
    let path = dir.join( format!("annual_precip__{}__{}.json", center_enc, ymd(date)));

    let sum_f32 = get_cached_annual_grid_precipitations( wx_config, grid, date, &path).await?;
    let sum_f64 = sum_f32.map( |l| l.to_f64() ); // NOTE - this has to be the same unit as we use in the compute_nfdrs_recs argument

    Ok( Grid::from_array2(sum_f64) )
}

/// use forecast data from fw21_grid plus already retrieved dir/dem.tif to compute micro grid wind with WindNinja
async fn compute_wind( ef: &ElmFire, config: &ElmfireConfig, fw21_grid: &UtmGrid<Fw21Data>, dir: &PathBuf) ->Result<()> {
    let wind_dir = dir.join("wn");
    ensure_writable_dir( &wind_dir)?;
    let n_steps = ef.n_hours as usize + 1;

    // extract HRRR conform DS for each hour from fw21_grid (note we don't need unique tif names since we place this into its own wind_dir)
    let wx_paths = wn_utm_grid_to_hrrr( &ef.name, &config.wx_model, &fw21_grid, &ef.base_date, n_steps, &wind_dir)?;

    run_wn_steps( ef, config, &wx_paths, &wind_dir, dir).await
}

/* #endregion actual weather inputs */

/* #region windninja support functions ****************************************************************************/

/// run WindNinja on the given (hourly) wx input paths and collect the huvw output into the ws.tif, wd.tif Elmfire inputs.
/// This does not care how the wx input paths were generated
async fn run_wn_steps (ef: &ElmFire, config: &ElmfireConfig, wx_paths: &[PathBuf], wind_dir: &PathBuf, dir: &PathBuf)->Result<()> {
    let wind_config: WindConfig = odin_wind::load_config( &config.wind_config_name)?;
    let n_steps = wx_paths.len();

    let dem_path = Arc::new( wind_dir.join( format!("{}.tif", ef.name)));
    create_symlink( Path::new("../dem.tif"), &dem_path.as_ref())?; // no need to copy

    let region = Arc::new(ef.name.clone());
    let wx_src = Arc::new( config.wx_model.clone());
    let mesh_res = config.wind_res_m as f64;

    // WindNinja uses the DEM tif to set the huvw output bbox so we should get it from there (wx input is a superset of this bbox)
    // the huvw size is not the same as the DEM though (which likely is of higher resolution) - we have to compute it
    let dem_ds = open_dataset(dem_path.as_ref())?;
    let ri = get_raster_info( &dem_ds)?;
    let bbox = BoundingBox::<f64>::from(&ri);
    let (w,h) = ((bbox.width() / mesh_res) as usize, (bbox.height() / mesh_res) as usize);

    let wn_out_basename = Arc::new( get_wn_out_basename( &ef.name, ef.base_date.clone(), &bbox, mesh_res, &config.wx_model));

    // create the ws.tif, wd.tif target datasets into which we collect the WindNinja huvw output
    let mut ws_ds = create_wn_ds( &dir.join("ws.tif"), w, h, n_steps)?;
    let mut wd_ds = create_wn_ds( &dir.join("wd.tif"), w, h, n_steps)?;

    for (n, p) in wx_paths.iter().enumerate() {
        let date = ef.base_date + hours(n as u64);
        let huvw_path = wn_output_path( wind_dir, region.as_ref(), &date, mesh_res, wx_src.as_ref());

        let wn_job = WnJob {
            region: region.clone(),
            date: date,
            step: 0, // irrelevant in this context
            mesh_res: mesh_res, // should be multiple of lf_res_m
            wind_height: 10.0,
            wx_src: wx_src.clone(),
            wx_path: Arc::new(p.clone()),
            dem_path: dem_path.clone(),
            wn_out_basename: wn_out_basename.clone()
        };

        // this might take a moment but since this is not recurring and grids are rather small we await it here
        // TODO - we could refactor this into its own task to make sure we only execute one WindNinja child proc at a time
        run_wn( &wind_config.windninja_cmd, wind_dir, &wn_job).await?;

        process_wn_output( &huvw_path, n+1, &mut ws_ds, &mut wd_ds)?;
    }

    Ok(())
}

fn create_wn_ds<P: AsRef<Path>> (path: P, w: usize, h: usize, n_steps: usize)->Result<GdalDataset> {
    let driver = get_gtiff_driver(); // needs to be non-async scoped as it is not Send
    Ok( create_dataset( &driver, path, w, h, n_steps, GdalDataType::Float32, Some(get_gtiff_create_opts()))? )
}

fn process_wn_output<P: AsRef<Path>> (huvw_path: P, band_no: usize, ws_ds: &mut GdalDataset, wd_ds: &mut GdalDataset)->Result<()> {
    // NOTE - WindNinja huvw has a speed band but it is only horizontal. For fire spread we probably want to use the
    // 3D windspeed, i.e. we have to read u,v,w components and compute the speed ourselves
    let huvw_ds = open_dataset(  huvw_path)?;

    if band_no == 1 { // we copy geo_transform and SRS from the WindNinja output
        copy_proj( &huvw_ds, ws_ds)?;
        copy_proj( &huvw_ds, wd_ds)?;
    }

    let n = find_rasterband_index( &huvw_ds, &[("", "name", Some("U"))]).ok_or(op_failed!("no U band"))? as usize;
    let wn_u: Vec<f32> = get_rasterband_as_vec( &huvw_ds, n)?;

    let n = find_rasterband_index( &huvw_ds, &[("", "name", Some("V"))]).ok_or(op_failed!("no V band"))? as usize;
    let wn_v: Vec<f32> = get_rasterband_as_vec( &huvw_ds, n)?;

    let n = find_rasterband_index( &huvw_ds, &[("", "name", Some("W"))]).ok_or(op_failed!("no W band"))? as usize;
    let wn_w: Vec<f32> = get_rasterband_as_vec( &huvw_ds, n)?;

    let n = find_rasterband_index( &huvw_ds, &[("", "name", Some("Direction"))]).ok_or(op_failed!("no Direction band"))? as usize;
    let wn_d: Vec<f32> = get_rasterband_as_vec( &huvw_ds, n)?;

    let len = wn_u.len();
    let mut ws: Vec<f32> = Vec::with_capacity(len);
    let mut wd: Vec<f32> = Vec::with_capacity(len);

    for i in 0..len {
        let spd = VelocityF32::new::<meter_per_second>( (wn_u[i].powi(2) + wn_v[i].powi(2) + wn_w[i].powi(2)).sqrt());
        ws.push(  spd.get::<mile_per_hour>());
        wd.push( wn_d[i])
    }

    set_rasterband_from_vec( ws_ds, band_no, ws)?;
    set_rasterband_from_vec( wd_ds, band_no, wd)?;

    Ok(())
}

/* #endregion windninja support functions */

/* #region landfire ***********************************************************************************************************/

pub async fn create_landfire_input (ef: &ElmFire, config: &ElmfireConfig)->Result<()> {
    let lf_src: GeoDs = odin_landfire::load_config( &config.lf_src_name)?;
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
