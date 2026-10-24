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

use std::path::{Path,PathBuf};
use chrono::{DateTime,Datelike,Timelike,Utc};
use uom::si::{f32::Velocity, velocity::{meter_per_second,mile_per_hour}};

use odin_common::{BoundingBox, fs::{ensure_writable_dir, path_as_str}};
use odin_gdal::{
    FillNoDataAlg, errors::OdinGdalError, GdalDataType, copy_proj, create_dataset, fill_nodata, find_rasterband_index, get_mem_driver,
    get_gtiff_create_opts, get_raster_info, get_rasterband_as_vec, open_dataset, set_rasterband_from_vec,
    gdal::{Dataset, spatial_ref::{AxisMappingStrategy, CoordTransform, SpatialRef}},
    warp::{ResampleAlg, SimpleWarpBuilder}
};
use odin_wind::{create_grid_csv, create_vector_csv, create_contour_json};
use crate::{WindField, Timestep, errors::{OdinElmfireError,Result,op_failed}};

const NODATA: f32 = -9999.0;

/// Elmfire wind speed input is for 20ft above ground
const EF_WIND_HEIGHT_M: f32 = 6.096;

/// approximate length of a latitude degree (we only need it to compute WGS84 cell sizes)
const METERS_PER_DEG: f64 = 111_320.0;

/// the UTM grid (at wind resolution) we compute wind fields for before we warp them to WGS84
struct WindGrid {
    bbox: BoundingBox<f64>,
    nx: usize,
    ny: usize
}

impl WindGrid {
    /// derive the grid from the run_dir/inputs/dem.tif bounding box (this is also what WindNinja uses for its output)
    fn from_dem<P: AsRef<Path>> (dem_path: P, wind_res_m: u32)->Result<Self> {
        let dem_ds = open_dataset( dem_path)?;
        let bbox = BoundingBox::<f64>::from( &get_raster_info( &dem_ds)?);
        let res = wind_res_m as f64;
        let nx = (bbox.width() / res).round() as usize;
        let ny = (bbox.height() / res).round() as usize;

        Ok( WindGrid { bbox, nx, ny } )
    }

    fn len (&self)->usize { self.nx * self.ny }
}

/// the wind components for a full hour, resampled to the WindGrid
struct HourlyWind {
    u: Vec<f32>,
    v: Vec<f32>,
    w: Option<Vec<f32>> // only WindNinja output has a vertical component
}

/// compute wx forecast based wind fields (grid/vector CSVs and contour GeoJSON), using run_dir/inputs/{dem,ws,wd}.tif
/// as basis (warped to WGS84) this has to create single time step huvw datasets for each of the ws,wd  bands (which are
/// for full hours i.e. have to be interpolated to timestep dates)
pub fn create_wx_windfields<P: AsRef<Path>> (run_dir: P, base_date: &DateTime<Utc>, timesteps: &[Timestep], wind_res_m: u32)->Result<Vec<WindField>> {
    let input_dir = run_dir.as_ref().join("inputs");
    let grid = WindGrid::from_dem( input_dir.join("dem.tif"), wind_res_m)?;

    // heights are terrain elevation + wind height (as in WindNinja huvw output)
    let dem_ds = warp_to_wind_grid( &open_dataset( input_dir.join("dem.tif"))?, &grid, ResampleAlg::Average)?;
    let h: Vec<f32> = get_rasterband_as_vec::<f32>( &dem_ds, 1)?.into_iter()
        .map( |e| if e == NODATA { NODATA } else { e + EF_WIND_HEIGHT_M })
        .collect();

    // ws/wd are in Elmfire input units (mph and meteorological direction the wind is coming from). Since we cannot
    // resample (or interpolate) directions we first convert into u,v components on the native wx grid
    let ws_ds = open_dataset( input_dir.join("ws.tif"))?;
    let wd_ds = open_dataset( input_dir.join("wd.tif"))?;
    let n_hours = ws_ds.raster_count();
    if wd_ds.raster_count() != n_hours { return Err( op_failed!("ws.tif and wd.tif band count mismatch")) }

    let (cols,rows) = ws_ds.raster_size();
    let mut hours: Vec<HourlyWind> = Vec::with_capacity( n_hours);

    for band in 1..=n_hours {
        let ws: Vec<f32> = get_rasterband_as_vec( &ws_ds, band)?;
        let wd: Vec<f32> = get_rasterband_as_vec( &wd_ds, band)?;

        let mut u: Vec<f32> = Vec::with_capacity( ws.len());
        let mut v: Vec<f32> = Vec::with_capacity( ws.len());
        for (s,d) in ws.iter().zip( wd.iter()) {
            if *s == NODATA || *d == NODATA {
                u.push( NODATA);
                v.push( NODATA);
            } else {
                let spd = Velocity::new::<mile_per_hour>(*s).get::<meter_per_second>();
                let rad = d.to_radians();
                u.push( -spd * rad.sin());
                v.push( -spd * rad.cos());
            }
        }

        let mut uv_ds = create_mem_ds( &ws_ds, cols, rows, 2)?;
        set_rasterband_from_vec( &mut uv_ds, 1, u)?;
        set_rasterband_from_vec( &mut uv_ds, 2, v)?;

        let uv_ds = warp_to_wind_grid( &uv_ds, &grid, ResampleAlg::Bilinear)?;
        hours.push( HourlyWind {
            u: get_rasterband_as_vec( &uv_ds, 1)?,
            v: get_rasterband_as_vec( &uv_ds, 2)?,
            w: None
        });
    }

    create_windfields( run_dir.as_ref(), base_date, timesteps, &grid, &dem_ds, &h, &hours)
}

/// compute WindNinja output based wind fields (grid/vector CSVs and contour GeoJSON). This uses the WindNinja generated
/// single timestep huvw datasets (warped to WGS84) but still has to interpolate to timestep dates
pub fn create_computed_windfields<P: AsRef<Path>> (run_dir: P, base_date: &DateTime<Utc>, timesteps: &[Timestep], wind_res_m: u32)->Result<Vec<WindField>> {
    let input_dir = run_dir.as_ref().join("inputs");
    let wind_dir = input_dir.join("wn");
    let grid = WindGrid::from_dem( input_dir.join("dem.tif"), wind_res_m)?;

    let mut hours: Vec<HourlyWind> = Vec::new();
    let mut h: Vec<f32> = Vec::new();
    let mut template: Option<Dataset> = None;

    // the WindNinja huvw output files are for consecutive full hours, starting at base_date
    let mut date = *base_date;
    while let Some(huvw_path) = find_wn_huvw_path( &wind_dir, &date)? {
        let src_ds = open_dataset( &huvw_path)?;
        let ih = band_index( &src_ds, "H")?;
        let iu = band_index( &src_ds, "U")?;
        let iv = band_index( &src_ds, "V")?;
        let iw = band_index( &src_ds, "W")?;

        // band order is preserved by warping
        let ds = warp_to_wind_grid( &src_ds, &grid, ResampleAlg::Bilinear)?;
        if h.is_empty() { h = get_rasterband_as_vec( &ds, ih)? } // terrain + wind height does not change over time

        hours.push( HourlyWind {
            u: get_rasterband_as_vec( &ds, iu)?,
            v: get_rasterband_as_vec( &ds, iv)?,
            w: Some( get_rasterband_as_vec( &ds, iw)?)
        });

        if template.is_none() { template = Some(ds) }
        date = date + chrono::Duration::hours(1);
    }

    let template = template.ok_or( op_failed!("no WindNinja output in {:?}", wind_dir))?;
    create_windfields( run_dir.as_ref(), base_date, timesteps, &grid, &template, &h, &hours)
}

/// find the WindNinja huvw output file for the given (full hour) date in wind_dir.
/// Note that WindNinja uses its own naming convention, e.g. Woods_10-09-2026_0100_150m_ncep_hrrr_conus_huvw.tif
fn find_wn_huvw_path (wind_dir: &Path, date: &DateTime<Utc>)->Result<Option<PathBuf>> {
    let date_spec = format!("_{:02}-{:02}-{:4}_{:02}{:02}_", date.month(), date.day(), date.year(), date.hour(), date.minute());

    for entry in std::fs::read_dir( wind_dir)? {
        let path = entry?.path();
        if let Some(fname) = path.file_name().and_then(|f| f.to_str()) {
            if fname.ends_with("_huvw.tif") && fname.contains( &date_spec) {
                return Ok( Some(path) )
            }
        }
    }
    Ok(None)
}

fn band_index (ds: &Dataset, name: &str)->Result<usize> {
    find_rasterband_index( ds, &[("", "name", Some(name))]).map( |i| i as usize).ok_or( op_failed!("no {} band", name))
}

/// the common part: interpolate the hourly wind components to timestep dates, create the (UTM) huvws dataset for each
/// timestep, warp it to WGS84 and generate the grid, vector and contour files from it
fn create_windfields (run_dir: &Path, base_date: &DateTime<Utc>, timesteps: &[Timestep], grid: &WindGrid,
                      template: &Dataset, h: &[f32], hours: &[HourlyWind])->Result<Vec<WindField>>
{
    if hours.is_empty() { return Err( op_failed!("no hourly wind data")) }

    let output_dir = run_dir.join("outputs");
    let scratch_dir = run_dir.join("scratch"); // for the intermediate WGS84 huvws GeoTIFFs
    ensure_writable_dir( &scratch_dir)?;
    let mesh_res = grid.bbox.width() / grid.nx as f64;
    let len = grid.len();
    let mut wfs: Vec<WindField> = Vec::with_capacity( timesteps.len());

    for (i,ts) in timesteps.iter().enumerate() {
        // get the bracketing hours and the interpolation factor for this timestep
        let dt = (ts.date - *base_date).num_seconds().max(0) as f32 / 3600.0;
        let k0 = (dt.floor() as usize).min( hours.len()-1);
        let k1 = (k0 + 1).min( hours.len()-1);
        let f = if k1 > k0 { dt - k0 as f32 } else { 0.0 };

        let (h0,h1) = (&hours[k0], &hours[k1]);
        let u = interpolate( &h0.u, &h1.u, f);
        let v = interpolate( &h0.v, &h1.v, f);
        let w = match (&h0.w, &h1.w) {
            (Some(w0),Some(w1)) => interpolate( w0, w1, f),
            _ => vec![0.0; len]
        };

        let s: Vec<f32> = (0..len).map( |j| {
            if u[j] == NODATA || v[j] == NODATA || w[j] == NODATA { NODATA }
            else { (u[j]*u[j] + v[j]*v[j] + w[j]*w[j]).sqrt() }
        }).collect();

        let mut huvws_ds = create_mem_ds( template, grid.nx, grid.ny, 5)?;
        set_rasterband_from_vec( &mut huvws_ds, 1, h.to_vec())?;
        set_rasterband_from_vec( &mut huvws_ds, 2, u)?;
        set_rasterband_from_vec( &mut huvws_ds, 3, v)?;
        set_rasterband_from_vec( &mut huvws_ds, 4, w)?;
        set_rasterband_from_vec( &mut huvws_ds, 5, s)?;

        let mut ds = warp_to_wgs84( &huvws_ds, mesh_res, scratch_dir.join( format!("wind_huvws_{i}_wgs84.tif")))?;
        fill_nodata( &mut ds, 50, 3, FillNoDataAlg::InverseDistance)?; // UTM -> WGS84 leaves NODATA around the edges

        let grid_fname = format!("wind_grid_{i}.csv");
        let vector_fname = format!("wind_vector_{i}.csv");
        let contour_fname = format!("wind_contour_{i}.json");

        // note those are stored gzipped (*.gz) but get served with the uncompressed name
        create_grid_csv( &output_dir.join( &grid_fname), &ds, &[1, 2, 3, 4])?;
        create_vector_csv( &output_dir.join( &vector_fname), &ds, &[1, 2, 3, 4], mesh_res)?;
        create_contour_json( &output_dir.join( &contour_fname), &ds, 5)?;

        wfs.push( WindField { date: ts.date, grid: grid_fname, vectors: vector_fname, contours: contour_fname });
    }

    Ok(wfs)
}

fn interpolate (a: &[f32], b: &[f32], f: f32)->Vec<f32> {
    a.iter().zip( b.iter()).map( |(a,b)| {
        if *a == NODATA || *b == NODATA { NODATA } else { a + (b - a) * f }
    }).collect()
}

/// create a Float32 MEM dataset with the projection and geo transform of `template` (which has to be of the same size)
fn create_mem_ds (template: &Dataset, cols: usize, rows: usize, n_bands: usize)->Result<Dataset> {
    let mut ds = create_dataset( &get_mem_driver(), "", cols, rows, n_bands, GdalDataType::Float32, None)?;
    copy_proj( template, &mut ds)?;
    for i in 1..=n_bands {
        ds.rasterband(i).and_then( |mut band| band.set_no_data_value( Some(NODATA as f64))).map_err( OdinGdalError::from)?;
    }
    Ok(ds)
}

/// resample `src_ds` into the (same SRS) WindGrid. This is used for the hourly wind/height inputs, which all cover the WindGrid bbox
fn warp_to_wind_grid (src_ds: &Dataset, grid: &WindGrid, alg: ResampleAlg)->Result<Dataset> {
    let bbox = &grid.bbox;
    let nodatas = vec![NODATA as f64; src_ds.raster_count()]; // SimpleWarpBuilder needs one per band

    let mut warper = SimpleWarpBuilder::new( src_ds, "")?;
    warper.set_tgt_format("MEM")?;
    warper.set_tgt_extent( bbox.west, bbox.south, bbox.east, bbox.north);
    warper.set_tgt_size( grid.nx as i32, grid.ny as i32);
    warper.set_resample_alg( alg);
    warper.set_data_type( GdalDataType::Float32);
    warper.set_src_nodatas( nodatas.clone());
    warper.set_tgt_nodatas( nodatas);

    Ok( warper.exec()? )
}

/// warp the UTM huvws dataset to a WGS84 GeoTIFF with cells that approximately have the wind resolution in both directions.
/// Note that the UTM grid does not cover the WGS84 bbox - uncovered pixels are initialized with nodata so that we can fill them later.
/// We keep the GeoTIFF in the scratch dir for inspection
fn warp_to_wgs84<P: AsRef<Path>> (src_ds: &Dataset, wind_res: f64, path: P)->Result<Dataset> {
    let mut src_srs = src_ds.spatial_ref().map_err( OdinGdalError::from)?;
    let mut tgt_srs = SpatialRef::from_epsg(4326).map_err( OdinGdalError::from)?;
    src_srs.set_axis_mapping_strategy( AxisMappingStrategy::TraditionalGisOrder);
    tgt_srs.set_axis_mapping_strategy( AxisMappingStrategy::TraditionalGisOrder); // lon,lat order

    let ri = get_raster_info( src_ds)?;
    let ct = CoordTransform::new( &src_srs, &tgt_srs).map_err( OdinGdalError::from)?;
    let [west,south,east,north] = ct.transform_bounds( &[ri.left, ri.bottom, ri.right, ri.top], 21).map_err( OdinGdalError::from)?;

    let lat_c = ((south + north) / 2.0).to_radians();
    let dy = wind_res / METERS_PER_DEG;
    let dx = dy / lat_c.cos();
    let nx = ((east - west) / dx).round() as i32;
    let ny = ((north - south) / dy).round() as i32;

    let nodatas = vec![NODATA as f64; src_ds.raster_count()];
    let create_opts = get_gtiff_create_opts();

    let mut warper = SimpleWarpBuilder::new( src_ds, path)?;
    warper.set_tgt_srs( &tgt_srs);
    warper.set_tgt_format("GTiff")?;
    warper.set_create_options( &create_opts);
    warper.set_tgt_extent( west, south, east, north);
    warper.set_tgt_size( nx, ny);
    warper.set_resample_alg( ResampleAlg::Bilinear);
    warper.set_src_nodatas( nodatas.clone());
    warper.set_tgt_nodatas( nodatas);

    Ok( warper.exec()? )
}
