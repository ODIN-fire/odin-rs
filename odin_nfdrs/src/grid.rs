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

/// support for computing NFDRS output fields (fuel moisture and fire danger indices) as hourly banded
/// geographic (EPSG:4326) GDAL datasets for a given GeoRect

use std::{path::{Path,PathBuf}, str::FromStr};
use chrono::{DateTime,Utc};
use serde::{Serialize,Deserialize};
use serde_json;
use odin_actor::prelude::warn;
use ndarray::Array2;
use strum::{Display, EnumIter, EnumString, IntoEnumIterator};
use uom::si::{f64::Length, length::meter, ratio::percent};

use odin_common::{
    BoundingBox,
    datetime::{days, full_hour, hours, ser_epoch_millis, de_from_epoch_millis},
    fs::ensure_writable_dir,
    geo::GeoRect,
    utm::{GeoRectMapping, UtmRect},
};
use odin_gdal::{
    gdal::spatial_ref::SpatialRef,
    errors::OdinGdalError,
    geods::GeoDs,
    CslStringList, get_driver_by_name, get_gtiff_create_opts, get_mem_driver, get_rasterband_as_vec, no_create_opts,
    open_dataset, set_rasterband_from_vec,
    grid::create_grid_ds_from_array2,
    warp::{ResampleAlg, SimpleWarpBuilder},
};
use odin_openmeteo::{
    MAX_POINTS, OpenMeteoConfig,
    ids::ModelId,
    queries::{get_annual_precip, get_fw21_grid},
};

use crate::{
    NfdrsConfig, NfdrsLocationClassification, NfdrsRecord, compute_nfdrs_recs,
    errors::{OdinNfdrsError, Result, op_failed},
};

/// number of days before base_date for which we retrieve weather data. NFDRS needs this history to compute
/// dead fuel moisture (>100h) and KBDI
pub const NFDRS_HISTORY_DAYS: u64 = 6;

/// nodata value for NFDRS grid datasets
pub const NODATA: f32 = -9999.0;

/// the NfdrsRecord fields we support as grid datasets. The string representation is used as the dataset file stem
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumString, EnumIter)]
pub enum NfdrsField {
    #[strum(serialize = "dfm_1h")]    Dfm1h,
    #[strum(serialize = "dfm_10h")]   Dfm10h,
    #[strum(serialize = "dfm_100h")]  Dfm100h,
    #[strum(serialize = "lfm_herb")]  LfmHerb,
    #[strum(serialize = "lfm_woody")] LfmWoody,
    #[strum(serialize = "bi")]        Bi,
    #[strum(serialize = "erc")]       Erc,
    #[strum(serialize = "sc")]        Sc,
    #[strum(serialize = "ic")]        Ic,
    #[strum(serialize = "gsi")]       Gsi,
    #[strum(serialize = "kbdi")]      Kbdi,
}

impl NfdrsField {
    /// the grid value for this field (fuel moisture in percent)
    pub fn value (&self, rec: &NfdrsRecord)->f32 {
        match self {
            NfdrsField::Dfm1h    => rec.dfm_1h.get::<percent>() as f32,
            NfdrsField::Dfm10h   => rec.dfm_10h.get::<percent>() as f32,
            NfdrsField::Dfm100h  => rec.dfm_100h.get::<percent>() as f32,
            NfdrsField::LfmHerb  => rec.lfm_herb.get::<percent>() as f32,
            NfdrsField::LfmWoody => rec.lfm_woody.get::<percent>() as f32,
            NfdrsField::Bi       => rec.bi,
            NfdrsField::Erc      => rec.erc,
            NfdrsField::Sc       => rec.sc,
            NfdrsField::Ic       => rec.ic,
            NfdrsField::Gsi      => rec.gsi,
            NfdrsField::Kbdi     => rec.kbdi as f32,
        }
    }

    pub fn file_name (&self)->String {
        format!("{}.tif", self)
    }

    pub fn all ()->Vec<NfdrsField> {
        NfdrsField::iter().collect()
    }

    /// is this a fuel moisture field (low values = high danger)
    pub fn is_moisture (&self)->bool {
        matches!( self, NfdrsField::Dfm1h | NfdrsField::Dfm10h | NfdrsField::Dfm100h | NfdrsField::LfmHerb | NfdrsField::LfmWoody)
    }

    pub fn unit (&self)->&'static str {
        if self.is_moisture() { "%" } else { "" }
    }

    pub fn description (&self)->&'static str {
        match self {
            NfdrsField::Dfm1h    => "1h dead fuel moisture",
            NfdrsField::Dfm10h   => "10h dead fuel moisture",
            NfdrsField::Dfm100h  => "100h dead fuel moisture",
            NfdrsField::LfmHerb  => "live herbaceous fuel moisture",
            NfdrsField::LfmWoody => "live woody fuel moisture",
            NfdrsField::Bi       => "burning index",
            NfdrsField::Erc      => "energy release component",
            NfdrsField::Sc       => "spread component",
            NfdrsField::Ic       => "ignition component",
            NfdrsField::Gsi      => "growing season index",
            NfdrsField::Kbdi     => "Keetch-Byram drought index",
        }
    }

    /// upper class bounds (exclusive) for color mapping. The last class is open ended, i.e. we have
    /// thresholds.len()+1 color classes
    // TODO - these are generic placeholder values that should be reviewed and/or made configurable
    pub fn thresholds (&self)->&'static [f32] {
        match self {
            NfdrsField::Dfm1h    => &[2.0, 4.0, 6.0, 8.0, 10.0, 15.0, 20.0],
            NfdrsField::Dfm10h   => &[3.0, 5.0, 7.0, 9.0, 12.0, 15.0, 20.0],
            NfdrsField::Dfm100h  => &[5.0, 8.0, 10.0, 12.0, 15.0, 20.0, 25.0],
            NfdrsField::LfmHerb  => &[30.0, 60.0, 90.0, 120.0, 150.0, 200.0, 250.0],
            NfdrsField::LfmWoody => &[60.0, 80.0, 100.0, 120.0, 140.0, 160.0, 200.0],
            NfdrsField::Bi       => &[10.0, 20.0, 30.0, 40.0, 60.0, 80.0, 100.0],
            NfdrsField::Erc      => &[10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0],
            NfdrsField::Sc       => &[5.0, 10.0, 15.0, 20.0, 30.0, 40.0, 60.0],
            NfdrsField::Ic       => &[10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 80.0],
            NfdrsField::Gsi      => &[0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.8],
            NfdrsField::Kbdi     => &[100.0, 200.0, 300.0, 400.0, 500.0, 600.0, 700.0],
        }
    }

    /// the color map entries for this field (from low to high values)
    pub fn color_map (&self)->Vec<ColorMapEntry> {
        let ts = self.thresholds();
        let unit = self.unit();
        let n = ts.len() + 1;
        let (clrs, descrs) = if self.is_moisture() { (&MOISTURE_CLRS, &MOISTURE_DESCRS) } else { (&INDEX_CLRS, &INDEX_DESCRS) };

        (0..n).map( |i| {
            let code = if i == 0 { format!("< {}{}", ts[0], unit) }
                       else if i == n-1 { format!("≥ {}{}", ts[i-1], unit) }
                       else { format!("{} - {}{}", ts[i-1], ts[i], unit) };
            let upper = if i < n-1 { ts[i] } else { f32::MAX };
            ColorMapEntry { upper, rgba: clrs[i], code, descr: descrs[i] }
        }).collect()
    }
}

/// a single color class of a field color map
#[derive(Debug, Clone)]
pub struct ColorMapEntry {
    pub upper: f32,      // exclusive upper bound of class values
    pub rgba: u32,       // color as 0xRRGGBBAA
    pub code: String,    // value range string
    pub descr: &'static str
}

impl ColorMapEntry {
    pub fn css_color (&self)->String { format!("#{:08x}", self.rgba) }
}

// 8 color classes from high danger (dry) to low danger (wet) - note PNG pixels are semi-transparent
const MOISTURE_CLRS: [u32;8] = [0x800026c0, 0xbd0026c0, 0xe31a1cc0, 0xfd8d3cc0, 0xfed976c0, 0xa1d99bc0, 0x41ab5dc0, 0x2171b5c0];
const MOISTURE_DESCRS: [&'static str;8] = ["extremely dry", "very dry", "dry", "moderately dry", "moderate", "moist", "wet", "very wet"];

// 8 color classes from low to high danger
const INDEX_CLRS: [u32;8] = [0x2171b5c0, 0x41ab5dc0, 0xa1d99bc0, 0xfed976c0, 0xfd8d3cc0, 0xe31a1cc0, 0xbd0026c0, 0x800026c0];
const INDEX_DESCRS: [&'static str;8] = ["very low", "low", "moderately low", "moderate", "moderately high", "high", "very high", "extreme"];

/// min/max/avg values of a field over all grid points within a bbox for a single hour
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct HourStats {
    pub min: f32,
    pub max: f32,
    pub avg: f32
}

/// hourly statistics of a field (entries are None if there were no valid grid points for that hour)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldStats {
    pub field: String,
    pub hourly: Vec<Option<HourStats>>
}

/// the result of create_nfdrs_grid_datasets
#[derive(Debug, Clone)]
pub struct NfdrsGridOutput {
    pub paths: Vec<PathBuf>,    // one EPSG:4326 GeoTIFF per field
    pub stats: Vec<FieldStats>  // one entry per field
}

/// compute the requested NFDRS fields for all grid points within `geo_rect` (spaced by `resolution`) and store
/// them as EPSG:4326 GeoTIFFs (one per field) in `dir`. Each dataset has `n_hours+1` raster bands (one per hour
/// starting at `base_date`), covers exactly the extent of `geo_rect` and has (approximately) `pixel_res` sized pixels.
/// The result also includes hourly min/max/avg values for all grid points inside of `geo_rect`
///
/// Weather (FW21) and annual precipitation data is obtained from Open-Meteo for the time interval
/// [`base_date` - NFDRS_HISTORY_DAYS, `base_date` + `n_hours`]. Note that NFDRS is executed once for each grid point
/// so this can take a while for larger grids.
pub async fn create_nfdrs_grid_datasets<P: AsRef<Path>> (
    nfdrs_config: &NfdrsConfig,
    wx_config: &OpenMeteoConfig,
    wx_model: ModelId,
    lf_src: &GeoDs,             // landfire source for slope/fuel classification of grid points
    geo_rect: &GeoRect,
    resolution: Length,          // cell size of the computation grid (should not be smaller than wx_model resolution)
    pixel_res: Length,           // pixel size of generated datasets
    base_date: &DateTime<Utc>,   // first band (should be a full hour)
    n_hours: u32,                // -> n_hours+1 hourly bands
    fields: &[NfdrsField],
    dir: P                       // output dir (also used for cached Open-Meteo JSON)
) -> Result<NfdrsGridOutput>
{
    if fields.is_empty() { return Err( op_failed!("no NFDRS fields requested")) }

    let dir = dir.as_ref().to_path_buf();
    ensure_writable_dir( &dir)?;

    let n_steps = n_hours as usize + 1;
    let start_date = *base_date - days( NFDRS_HISTORY_DAYS);
    let end_date = *base_date + hours( n_hours as u64);

    //--- compute grid dimensions so that the UTM grid fully covers geo_rect
    let res = resolution.get::<meter>();
    let utm_rect = UtmRect::from_geo_rect( geo_rect, GeoRectMapping::InteriorGeo)?;
    let nx2 = (utm_rect.width() / res / 2.0).ceil() as usize; // grid width is (2*nx2+1) * res
    let ny2 = (utm_rect.height() / res / 2.0).ceil() as usize;
    if (2*nx2 + 1) * (2*ny2 + 1) >= MAX_POINTS {
        return Err( op_failed!("too many grid points: {}x{} (max {})", 2*nx2+1, 2*ny2+1, MAX_POINTS))
    }

    //--- the cached weather data acquisition
    let geo_center = geo_rect.center();
    let fw21_grid = get_fw21_grid( wx_config, wx_model, &dir, &geo_center, resolution, nx2, ny2, &start_date, &end_date).await?;
    let precip_grid = get_annual_precip( wx_config, &dir, &fw21_grid, &geo_center, base_date).await?;

    //--- run NFDRS for each grid point and collect requested field values (one Array2 per hour)
    let (ny,nx) = fw21_grid.dim();
    let mut field_vals: Vec<Vec<Array2<f32>>> = vec![ vec![ Array2::from_elem( (ny,nx), NODATA); n_steps]; fields.len()];
    let mut inside: Array2<bool> = Array2::from_elem( (ny,nx), false); // grid points inside of geo_rect (for stats)

    for j in 0..ny {
        for i in 0..nx {
            let grid_point = (j,i); // ndarray uses row major = (row,col)
            let pos = fw21_grid.geo_pos_at( i, j)?; // we use (col,row)
            inside[grid_point] = geo_rect.contains_point( &pos);

            if let Some(fw21) = fw21_grid.at( i, j)? {  // note this is (col,row)
                let loc_cls = NfdrsLocationClassification::for_location( lf_src, &pos).await?;
                let annual_precip = precip_grid.at( i, j); // note this is (col,row)

                let nfdrs_recs = compute_nfdrs_recs( nfdrs_config, &pos, fw21, annual_precip, &loc_cls, wx_model, base_date).await?;

                for (k,rec) in nfdrs_recs.iter().take(n_steps).enumerate() {
                    for (f,field) in fields.iter().enumerate() {
                        field_vals[f][k][grid_point] = field.value( rec);
                    }
                }
            }
        }
    }

    //--- compute the hourly bbox statistics. If the bbox is smaller than a grid cell we use all grid points
    if !inside.iter().any( |b| *b) { inside.fill( true) }
    let stats: Vec<FieldStats> = fields.iter().zip( field_vals.iter()).map( |(field, vals)| {
        FieldStats { field: field.to_string(), hourly: vals.iter().map( |arr| get_hour_stats( arr, &inside)).collect() }
    }).collect();

    //--- create the EPSG:4326 datasets (no await points from here on since GDAL Driver is not Send)
    let epsg = fw21_grid.utm_rect.epsg();
    let (x_min, y_max) = (fw21_grid.utm_rect.west, fw21_grid.utm_rect.north);
    let (x_res, y_res) = (fw21_grid.x_resolution(), fw21_grid.y_resolution());
    let pixel_res = pixel_res.get::<meter>();

    let mut paths: Vec<PathBuf> = Vec::with_capacity( fields.len());
    for (field, vals) in fields.iter().zip( field_vals.into_iter()) {
        let path = dir.join( field.file_name());
        create_wgs84_ds( &path, geo_rect, epsg, x_min, y_max, nx, ny, x_res, y_res, pixel_res, vals)?;
        paths.push( path);
    }

    Ok( NfdrsGridOutput { paths, stats } )
}

fn get_hour_stats (vals: &Array2<f32>, inside: &Array2<bool>)->Option<HourStats> {
    let mut min = f32::MAX;
    let mut max = f32::MIN;
    let mut sum = 0.0f64;
    let mut n = 0;

    for (v, is_inside) in vals.iter().zip( inside.iter()) {
        if *is_inside && *v != NODATA && v.is_finite() {
            min = min.min( *v);
            max = max.max( *v);
            sum += *v as f64;
            n += 1;
        }
    }

    if n > 0 { Some( HourStats { min, max, avg: (sum / n as f64) as f32 }) } else { None }
}

/// create a GeoTIFF with the given geo_rect extent in EPSG:4326 from hourly band values on a UTM grid
fn create_wgs84_ds (
    path: &PathBuf, geo_rect: &GeoRect,
    epsg: u32, x_min: f64, y_max: f64, nx: usize, ny: usize, x_res: f64, y_res: f64, pixel_res: f64,
    vals: Vec<Array2<f32>>
) -> Result<()>
{
    let n_bands = vals.len();

    // the intermediate UTM dataset is only kept in memory
    let mem_driver = get_mem_driver();
    let utm_ds = create_grid_ds_from_array2::<f32,_>( &mem_driver, "", epsg, x_min, y_max, nx, ny, x_res, y_res, vals, &no_create_opts())?;

    let tgt_srs = SpatialRef::from_epsg(4326).map_err( OdinGdalError::from)?;
    let create_opts = get_gtiff_create_opts();
    let nodatas = vec![ NODATA as f64; n_bands];

    // we have to set the target size explicitly - otherwise GDAL keeps its own pixel size and rounds the number of
    // pixels, i.e. the target extent would not exactly match geo_rect
    let (tgt_width, tgt_height) = get_wgs84_size( geo_rect, pixel_res, pixel_res);

    let mut warper = SimpleWarpBuilder::new( &utm_ds, path)?;
    warper.set_tgt_srs( &tgt_srs);
    warper.set_tgt_extent_from_rect( geo_rect);         // the result covers exactly the requested rect
    warper.set_tgt_size( tgt_width, tgt_height);
    // CubicSpline (B-spline) gives a more continuous look than the coarse grid cells. Note it smooths (does not interpolate)
    // grid point values but does not overshoot. Nodata source pixels are excluded via set_src_nodatas()
    warper.set_resample_alg( ResampleAlg::CubicSpline);
    warper.set_src_nodatas( nodatas.clone());
    warper.set_tgt_nodatas( nodatas);
    warper.set_tgt_format( "GTiff")?;
    warper.set_create_options( &create_opts);
    warper.exec()?;

    Ok(())
}

/// number of EPSG:4326 pixels/lines for geo_rect that approximately correspond to the given (UTM) pixel resolutions in meters
fn get_wgs84_size (geo_rect: &GeoRect, x_res: f64, y_res: f64)->(i32,i32) {
    const M_PER_DEG: f64 = 111_320.0; // meters per degree latitude (and longitude at the equator)

    let lat = geo_rect.center().latitude_degrees();
    let dlon = x_res / (M_PER_DEG * lat.to_radians().cos());
    let dlat = y_res / M_PER_DEG;

    let w = geo_rect.east().degrees() - geo_rect.west().degrees();
    let h = geo_rect.north().degrees() - geo_rect.south().degrees();

    ( ((w / dlon).round() as i32).max(1), ((h / dlat).round() as i32).max(1) )
}

/* #region PNG products ********************************************************************************************************/

/// the filename of the PNG image for the given field and hour (band index starting at 0)
pub fn png_file_name (field: NfdrsField, hour: usize)->String {
    format!("{}_{}.png", field, hour)
}

/// create one (color mapped) PNG image per raster band of the given field dataset. Images are stored in `dir` and
/// named `<field>_<hour>.png`. Nodata pixels are transparent. Pixel colors are the color map class colors, i.e.
/// images show discrete color classes
pub fn create_field_pngs<P: AsRef<Path>, U: AsRef<Path>> (tif_path: P, field: NfdrsField, dir: U)->Result<Vec<PathBuf>> {
    let cmap = field.color_map();
    create_pngs( tif_path, field, dir, |v| cmap.iter().find( |ce| v < ce.upper).unwrap_or( &cmap[cmap.len()-1]).rgba)
}

/// create one (color mapped) PNG image per raster band of the given field dataset (see create_field_pngs()). Pixel colors
/// are linearly interpolated between the color map class colors, which are anchored at the class centers (the open
/// ended first and last classes are anchored half a neighbor class width beyond their threshold). This gives a
/// continuous appearance while values at class centers still have the exact color map class color
pub fn create_interpolated_field_pngs<P: AsRef<Path>, U: AsRef<Path>> (tif_path: P, field: NfdrsField, dir: U)->Result<Vec<PathBuf>> {
    let anchors = get_color_anchors( field);
    create_pngs( tif_path, field, dir, |v| interpolate_rgba( v, &anchors))
}

/// (value,rgba) pairs at the class centers of the field color map, sorted by value
fn get_color_anchors (field: NfdrsField)->Vec<(f32,u32)> {
    let ts = field.thresholds();
    let cmap = field.color_map();
    let n = ts.len(); // number of thresholds, cmap.len() == n+1

    cmap.iter().enumerate().map( |(i,ce)| {
        let v = if n == 1 {
            ts[0] + if i == 0 { -0.5 } else { 0.5 } // degenerated - no class widths
        } else if i == 0 {
            ts[0] - (ts[1] - ts[0]) / 2.0
        } else if i == n {
            ts[n-1] + (ts[n-1] - ts[n-2]) / 2.0
        } else {
            (ts[i-1] + ts[i]) / 2.0
        };
        (v, ce.rgba)
    }).collect()
}

/// linear interpolation of RGBA colors between the anchors that enclose v (clamped to first/last anchor color)
fn interpolate_rgba (v: f32, anchors: &[(f32,u32)])->u32 {
    let (v0, c0) = anchors[0];
    if v <= v0 { return c0 }

    for w in anchors.windows(2) {
        let (lo, clo) = w[0];
        let (hi, chi) = w[1];
        if v <= hi {
            let t = if hi > lo { (v - lo) / (hi - lo) } else { 1.0 };
            let mix = |shift: u32| {
                let a = ((clo >> shift) & 0xff) as f32;
                let b = ((chi >> shift) & 0xff) as f32;
                ((a + (b - a) * t).round() as u32) << shift
            };
            return mix(24) | mix(16) | mix(8) | mix(0)
        }
    }

    anchors[anchors.len()-1].1
}

/// the common PNG creation for all band values of a field dataset, using the provided value->rgba function
fn create_pngs<P,U,F> (tif_path: P, field: NfdrsField, dir: U, rgba_of: F)->Result<Vec<PathBuf>>
    where P: AsRef<Path>, U: AsRef<Path>, F: Fn(f32)->u32
{
    let ds = open_dataset( tif_path)?;
    let (w,h) = ds.raster_size();
    let n_bands = ds.raster_count() as usize;

    let mem_driver = get_mem_driver();
    let png_driver = get_driver_by_name("PNG").ok_or( op_failed!("no PNG driver"))?;

    let mut paths: Vec<PathBuf> = Vec::with_capacity( n_bands);
    for b in 0..n_bands {
        let vals: Vec<f32> = get_rasterband_as_vec( &ds, b+1)?;
        let len = vals.len();
        let mut rs: Vec<u8> = vec![0; len];
        let mut gs: Vec<u8> = vec![0; len];
        let mut bs: Vec<u8> = vec![0; len];
        let mut als: Vec<u8> = vec![0; len]; // transparent unless we have a value

        for (i,v) in vals.iter().enumerate() {
            if *v != NODATA && v.is_finite() {
                let rgba = rgba_of( *v);
                rs[i] = (rgba >> 24) as u8;
                gs[i] = (rgba >> 16) as u8;
                bs[i] = (rgba >> 8) as u8;
                als[i] = rgba as u8;
            }
        }

        // PNG only supports create_copy so we have to create the image in memory first
        let mut img_ds = mem_driver.create_with_band_type::<u8,_>( "", w, h, 4).map_err( OdinGdalError::from)?;
        set_rasterband_from_vec( &mut img_ds, 1, rs)?;
        set_rasterband_from_vec( &mut img_ds, 2, gs)?;
        set_rasterband_from_vec( &mut img_ds, 3, bs)?;
        set_rasterband_from_vec( &mut img_ds, 4, als)?;

        let path = dir.as_ref().join( png_file_name( field, b));
        img_ds.create_copy( &png_driver, &path, &CslStringList::new()).map_err( OdinGdalError::from)?;
        paths.push( path);
    }

    Ok( paths )
}

/* #endregion PNG products */

/* #region requests and runs **********************************************************************************************************/

/// a client request to compute NFDRS grid products for a given area, base date and duration.
/// This is also the message we send to the NfdrsActor. Note that `id` and `res_m` are derived values that are
/// set by the actor through `normalize()` (based on its configured wx_model)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct NfdrsRequest {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub bbox: BoundingBox<f64>,   // geographic bounds in degrees
    #[serde(serialize_with="ser_epoch_millis", deserialize_with="de_from_epoch_millis")]
    pub date: DateTime<Utc>,     // base date of computation (first hourly product), normalized to full hour
    pub n_hours: u32,            // number of forecast hours (we produce n_hours+1 hourly products)
    #[serde(default)]
    pub res_m: f64,              // grid point distance in meters (derived from wx_model resolution)
    #[serde(default)]
    pub update: bool,            // (future) recurring re-computation when new forecasts become available
}

impl NfdrsRequest {
    /// set derived fields (date rounded to full hour, grid resolution and id)
    pub fn normalize (&mut self, res_m: f64) {
        self.date = full_hour( &self.date);
        self.res_m = res_m;
        self.id = Self::get_id( &self.name, &self.bbox, &self.date, self.n_hours, self.res_m);
    }

    /// do we have a non-empty bbox within UTM latitude limits (our computation grid is UTM based)
    pub fn has_valid_bbox (&self)->bool {
        let b = &self.bbox;
        b.west < b.east && b.south < b.north && b.south >= -80.0 && b.north <= 84.0
    }

    pub fn geo_rect (&self)->GeoRect {
        GeoRect::from_bbox_degrees( &self.bbox)
    }

    /// the id is a valid directory name that is unique for (bbox, date, n_hours, res_m)
    /// the name prefix makes run directories easier to recognize
    pub fn get_id (name: &str, bbox: &BoundingBox<f64>, date: &DateTime<Utc>, n_hours: u32, res_m: f64)->String {
        let enc = |v: f64| format!("{:.3}", v).replace('.', "_");
        format!("{}__{}__{}_{}_{}_{}__{:.0}m__{}h", Self::name_prefix(name), date.format("%Y-%m-%d-%H%MZ"),
                enc(bbox.west), enc(bbox.south), enc(bbox.east), enc(bbox.north), res_m, n_hours)
    }

    /// a file system safe version of the request name. Since we use "__" as id field separator we map all
    /// chars other than [A-Za-z0-9.-] (including '_') to '-'
    fn name_prefix (name: &str)->String {
        const MAX_LEN: usize = 32;
        let s: String = name.trim().chars().take(MAX_LEN)
            .map( |c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '-' })
            .collect();
        let s = s.trim_matches(|c| c == '-' || c == '.');
        if s.is_empty() { "area".to_string() } else { s.to_string() }
    }

    pub fn dir<P: AsRef<Path>> (&self, root_dir: P)->PathBuf {
        root_dir.as_ref().join( &self.id)
    }
}

/// the results of a successful NfdrsRequest computation. PNG products are named `<field>_<hour>.png` (see png_file_name())
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct NfdrsOutput {
    pub pixel_res_m: f64,        // (approximate) pixel size of generated datasets
    pub stats: Vec<FieldStats>,  // hourly min/max/avg values for each computed field
}

impl NfdrsOutput {
    pub fn fields (&self)->Vec<&str> {
        self.stats.iter().map( |s| s.field.as_str()).collect()
    }
}

/// the wrapper for a NfdrsRequest and its (eventual) result. A `None` result means the request is still pending.
/// Runs are persisted as JSON manifests in their own directories (which also hold the products)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NfdrsRun {
    pub request: NfdrsRequest,
    pub result: Option<std::result::Result<NfdrsOutput,String>>
}

pub const NFDRS_RUN_MANIFEST: &'static str = "nfdrs_run.json";

impl NfdrsRun {
    pub fn new (request: NfdrsRequest)->Self {
        NfdrsRun { request, result: None }
    }

    pub fn id (&self)->&str { self.request.id.as_str() }

    pub fn is_pending (&self)->bool { self.result.is_none() }

    pub fn dir<P: AsRef<Path>> (&self, root_dir: P)->PathBuf { self.request.dir( root_dir) }

    pub fn save<P: AsRef<Path>> (&self, root_dir: P)->Result<()> {
        let dir = self.dir( root_dir);
        ensure_writable_dir( &dir)?;
        let json = serde_json::to_string_pretty( self)?;
        std::fs::write( dir.join( NFDRS_RUN_MANIFEST), json)?;
        Ok(())
    }

    pub fn load<P: AsRef<Path>> (run_dir: P)->Result<Self> {
        let json = std::fs::read_to_string( run_dir.as_ref().join( NFDRS_RUN_MANIFEST))?;
        Ok( serde_json::from_str( &json)? )
    }

    /// load all runs stored in sub-directories of `root_dir`
    pub fn load_all<P: AsRef<Path>> (root_dir: P)->Vec<Self> {
        let mut runs: Vec<Self> = Vec::new();
        if let Ok(entries) = std::fs::read_dir( root_dir.as_ref()) {
            for e in entries.flatten() {
                let path = e.path();
                if path.is_dir() {
                    match Self::load( &path) {
                        Ok(run) => runs.push( run),
                        Err(e) => warn!("ignoring invalid NFDRS run in {:?}: {}", path, e)
                    }
                }
            }
        }
        runs
    }

    pub fn remove_dir<P: AsRef<Path>> (&self, root_dir: P)->Result<()> {
        let dir = self.dir( root_dir);
        if dir.is_dir() { std::fs::remove_dir_all( &dir)? }
        Ok(())
    }

    /// the client (JSON) representation of this run
    pub fn to_msg (&self)->NfdrsRunMsg<'_> {
        let (status, output, error) = match &self.result {
            None => ("pending", None, None),
            Some(Ok(output)) => ("done", Some(output), None),
            Some(Err(e)) => ("failed", None, Some(e.as_str()))
        };
        NfdrsRunMsg { request: &self.request, status, output, error }
    }
}

/// what we send to clients for each NfdrsRun
#[derive(Debug, Serialize)]
pub struct NfdrsRunMsg<'a> {
    #[serde(flatten)]
    pub request: &'a NfdrsRequest,
    pub status: &'static str,
    pub output: Option<&'a NfdrsOutput>,
    pub error: Option<&'a str>
}

/// compute all NFDRS fields (datasets, PNG products and stats) for the given request. This is potentially long running
pub async fn compute_nfdrs_output<P: AsRef<Path>> (
    request: &NfdrsRequest, root_dir: P,
    nfdrs_config: &NfdrsConfig, wx_config: &OpenMeteoConfig, wx_model: ModelId, lf_src: &GeoDs
) -> Result<NfdrsOutput>
{
    let dir = request.dir( root_dir);
    let fields = NfdrsField::all();
    let geo_rect = request.geo_rect();
    let resolution = Length::new::<meter>( request.res_m);
    let pixel_res = Length::new::<meter>( nfdrs_config.pixel_res_m);

    let grid_output = create_nfdrs_grid_datasets( nfdrs_config, wx_config, wx_model, lf_src,
                                                  &geo_rect, resolution, pixel_res, &request.date, request.n_hours, &fields, &dir).await?;

    for (field, tif_path) in fields.iter().zip( grid_output.paths.iter()) {
        create_interpolated_field_pngs( tif_path, *field, &dir)?; // or create_field_pngs() for discrete color classes
    }

    Ok( NfdrsOutput { pixel_res_m: nfdrs_config.pixel_res_m, stats: grid_output.stats } )
}

/* #endregion requests and runs */

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_request_id () {
        let bbox = BoundingBox::new( -122.302, 37.227, -122.093, 37.368);
        let date = Utc.with_ymd_and_hms( 2026, 10, 15, 20, 0, 0).unwrap();

        let id = NfdrsRequest::get_id( "bbox/Big Sur: north_side", &bbox, &date, 6, 3000.0);
        println!("{id}");
        assert_eq!( id, "bbox-Big-Sur--north-side__2026-10-15-2000Z__-122_302_37_227_-122_093_37_368__3000m__6h");

        assert_eq!( NfdrsRequest::name_prefix("  //  "), "area");
        assert_eq!( NfdrsRequest::name_prefix("Chimney Fire 2026.10"), "Chimney-Fire-2026.10");
    }

    #[test]
    fn test_color_interpolation () {
        let field = NfdrsField::Erc; // thresholds 10,20,..70
        let anchors = get_color_anchors( field);
        let cmap = field.color_map();
        assert_eq!( anchors.len(), cmap.len());
        assert_eq!( anchors[0].0, 5.0);
        assert_eq!( anchors[1].0, 15.0);
        assert_eq!( anchors[7].0, 75.0);

        // class centers have exact class colors, values beyond first/last anchors are clamped
        for (i,(v,_)) in anchors.iter().enumerate() {
            assert_eq!( interpolate_rgba( *v, &anchors), cmap[i].rgba);
        }
        assert_eq!( interpolate_rgba( -100.0, &anchors), cmap[0].rgba);
        assert_eq!( interpolate_rgba( 1000.0, &anchors), cmap[7].rgba);

        // threshold value is the midpoint between the two adjacent class colors
        let c = interpolate_rgba( 10.0, &anchors);
        let (c0, c1) = (cmap[0].rgba, cmap[1].rgba);
        for shift in [24u32, 16, 8, 0] {
            let (a, b, m) = ((c0 >> shift) & 0xff, (c1 >> shift) & 0xff, (c >> shift) & 0xff);
            assert!( m.abs_diff( (a + b + 1) / 2) <= 1, "channel {shift}: {a} {b} -> {m}");
        }
    }
}
