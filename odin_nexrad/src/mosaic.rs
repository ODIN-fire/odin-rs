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

//! server side overview mosaic of all stations, which is periodically computed from the latest product time step of
//! each station and distributed as GeoJSON contour polygons (reflectivity bands) for low resolution / wide area display.
//! The per-station WMS layers remain the source for high resolution display and value lookup.
//!
//! A mosaic is computed in five steps:
//!
//! 1. reduce the (high resolution) station grids to about the mosaic resolution by averaging linear reflectivity
//!    (Z = 10^(dBZ/10), i.e. the physical area mean, which is dominated by strong echoes)
//! 2. blend the reduced station grids onto the mosaic grid, preferring the station that is nearest to each cell.
//!    Stations that are less than `blend_m` farther away than the nearest one contribute with linearly decreasing
//!    weights, and weights also taper off over `blend_m` towards the max range of a station. This avoids seams
//!    (value discontinuities that would show up as contour edges) where the preferred station changes
//! 3. smooth the mosaic with a gaussian filter (sigma `smooth_m`)
//! 4. contour the mosaic into polygons for each reflectivity band between configured levels (GDAL)
//! 5. simplify polygons (`simplify_m` tolerance) and drop small ones (`min_area_km2`)
//!
//! Blending and smoothing are done in dBZ, with "no echo" being `levels[0] - NO_ECHO_OFFSET` (this determines how much
//! smoothing shrinks the lowest band). Range folded cells are treated as missing data, i.e. other stations can fill in

use std::{collections::HashMap, path::{Path,PathBuf}, ptr::null_mut, sync::Arc, thread, time::{Duration, Instant}};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use gdal::{
    Dataset, DriverManager, cpl::CslStringList, raster::Buffer, spatial_ref::SpatialRef,
    vector::{Geometry, LayerAccess, LayerOptions, OGRFieldType}
};
use gdal_sys::OGRwkbGeometryType;
use gdal::vector::geometry_type_flatten;

use odin_common::datetime::deserialize_duration;

use crate::{
    NexradEntry, NexradProduct, Station, REF_RANGE_FOLDED, code_to_dbz, date_tag, dbz_to_code, parse_date_tag,
    errors::{Result, op_failed},
    product::{MAX_RANGE_KM, METERS_PER_DEG_LAT, ProductGrid, cell_size_deg, reflectivity_palette, station_extent},
};

/// "no echo" value of the mosaic below the lowest contour level \[dBZ\]
pub const NO_ECHO_OFFSET: f32 = 5.0;

/* #region config *****************************************************************************************************/

/// configuration of the overview mosaic
#[derive(Debug,Clone,Deserialize)]
pub struct MosaicConfig {
    /// how often we check if there were updates since the last mosaic
    #[serde(deserialize_with="deserialize_duration")]
    pub interval: Duration,

    /// only the latest time steps of stations that are not older than this (relative to the newest time step of all
    /// stations) are used
    #[serde(deserialize_with="deserialize_duration")]
    pub max_age: Duration,

    /// number of mosaics (overview time steps) we keep per product
    #[serde(default="default_max_mosaics")]
    pub max_mosaics: usize,

    /// mosaic grid cell size \[m\]
    #[serde(default="default_resolution_m")]
    pub resolution_m: f64,

    /// sigma of the gaussian smoothing filter \[m\] (0 means no smoothing)
    #[serde(default="default_smooth_m")]
    pub smooth_m: f64,

    /// width of the transition zone between stations and towards the max range of stations \[m\]
    #[serde(default="default_blend_m")]
    pub blend_m: f64,

    /// tolerance for polygon simplification \[m\] (0 means no simplification)
    #[serde(default="default_simplify_m")]
    pub simplify_m: f64,

    /// polygons and holes smaller than this are dropped \[km²\]
    #[serde(default="default_min_area_km2")]
    pub min_area_km2: f64,

    /// lower bounds of reflectivity bands \[dBZ\] (ascending)
    #[serde(default="default_levels")]
    pub levels: Vec<f32>,
}

fn default_max_mosaics()->usize { 12 }
fn default_resolution_m()->f64 { 1000.0 }
fn default_smooth_m()->f64 { 1500.0 }
fn default_blend_m()->f64 { 20000.0 }
fn default_simplify_m()->f64 { 250.0 }
fn default_min_area_km2()->f64 { 4.0 }
fn default_levels()->Vec<f32> { vec![ 10.0, 20.0, 30.0, 40.0, 50.0 ] }

impl MosaicConfig {
    pub fn no_echo_dbz (&self)->f32 {
        self.levels.first().cloned().unwrap_or( 0.0) - NO_ECHO_OFFSET
    }
}

/* #endregion config */

/* #region station grids **********************************************************************************************/

/// a station product grid that is reduced to about the mosaic resolution. Values are dBZ, `no_echo` for cells without
/// echo and NaN for missing data (mostly range folded)
#[derive(Debug,Clone)]
pub struct StationGrid {
    pub station: String,
    pub scan_time: DateTime<Utc>,
    pub latitude: f64,
    pub longitude: f64,
    pub max_range_m: f64,
    pub grid: ProductGrid,
    pub dbz: Vec<f32>,
}

impl StationGrid {
    /// read and reduce the product GeoTIFF of an entry
    pub fn from_entry (entry: &NexradEntry, station: &Station, target_res_m: f64, no_echo: f32)->Result<Self> {
        let ds = Dataset::open( &entry.path)?;
        let gt = ds.geo_transform()?;
        let (width, height) = ds.raster_size();
        let grid = ProductGrid { west: gt[0], north: gt[3], res_lon: gt[1], res_lat: -gt[5], width, height };
        let codes = ds.rasterband(1)?.read_band_as::<u8>()?.into_shape_and_vec().1;
        Self::from_codes( station, entry.scan_time, &codes, &grid, target_res_m, no_echo)
    }

    /// reduce a product code grid by an integer factor that gets us closest to `target_res_m`. Cell values are the
    /// mean of linear reflectivity (no echo counting as zero) or NaN if most of the cells are range folded
    pub fn from_codes (station: &Station, scan_time: DateTime<Utc>, codes: &[u8], grid: &ProductGrid, target_res_m: f64, no_echo: f32)->Result<Self> {
        if codes.len() != grid.width * grid.height { return Err( op_failed!("grid size mismatch")) }
        let k = ((target_res_m / (grid.res_lat * METERS_PER_DEG_LAT)).round() as usize).max(1);
        let rgrid = ProductGrid {
            west: grid.west, north: grid.north, res_lon: grid.res_lon * k as f64, res_lat: grid.res_lat * k as f64,
            width: grid.width.div_ceil(k), height: grid.height.div_ceil(k)
        };

        // linear Z for each code (no echo and range folded are 0)
        let z_lut: Vec<f32> = (0..=255u8).map( |c| code_to_dbz(c).map( |dbz| 10f32.powf( dbz / 10.0)).unwrap_or(0.0)).collect();

        let mut dbz = vec![ no_echo; rgrid.width * rgrid.height];
        parallel_rows( &mut dbz, rgrid.width, |rrow, out| {
            for (rcol, v) in out.iter_mut().enumerate() {
                let (mut z_sum, mut n, mut n_rf) = (0.0f32, 0usize, 0usize);
                for row in rrow*k..((rrow+1)*k).min( grid.height) {
                    let row_codes = &codes[row*grid.width..(row+1)*grid.width];
                    for &c in &row_codes[rcol*k..((rcol+1)*k).min( grid.width)] {
                        if c == REF_RANGE_FOLDED { n_rf += 1 } else { z_sum += z_lut[c as usize] }
                        n += 1;
                    }
                }
                *v = if 2 * n_rf > n {
                    f32::NAN
                } else if z_sum > 0.0 {
                    (10.0 * (z_sum / (n - n_rf) as f32).log10()).max( no_echo)
                } else {
                    no_echo
                };
            }
        });

        // the grid extent is computed from the max sweep range (see `product::compute_product`)
        let max_range_m = grid.height as f64 * grid.res_lat * METERS_PER_DEG_LAT / 2.0;
        Ok( StationGrid { station: station.id.clone(), scan_time, latitude: station.latitude, longitude: station.longitude, max_range_m, grid: rgrid, dbz })
    }

    /// value at a position (None if outside of the grid or missing)
    pub fn value_at (&self, latitude: f64, longitude: f64)->Option<f32> {
        let idx = self.grid.cell_index( latitude, longitude)?;
        let v = self.dbz[idx];
        if v.is_nan() { None } else { Some(v) }
    }

    /// approximate (equirectangular) distance from the station \[m\]
    pub fn distance_m (&self, latitude: f64, longitude: f64)->f64 {
        let dy = (latitude - self.latitude) * METERS_PER_DEG_LAT;
        let dx = (longitude - self.longitude) * METERS_PER_DEG_LAT * self.latitude.to_radians().cos();
        (dx*dx + dy*dy).sqrt()
    }
}

/* #endregion station grids */

/* #region mosaic computation *****************************************************************************************/

/// the mosaic grid that covers the max range of all given stations, with cells of `resolution_m` that are square at
/// the center latitude
pub fn mosaic_grid (stations: &[Station], resolution_m: f64)->Result<ProductGrid> {
    let mut ext = [f64::MAX, f64::MAX, f64::MIN, f64::MIN]; // w,s,e,n
    for s in stations {
        let e = station_extent( s, MAX_RANGE_KM)?;
        ext = [ ext[0].min(e[0]), ext[1].min(e[1]), ext[2].max(e[2]), ext[3].max(e[3]) ];
    }
    if ext[0] >= ext[2] { return Err( op_failed!("no mosaic stations")) }

    let (res_lon, res_lat) = cell_size_deg( (ext[1] + ext[3]) / 2.0, resolution_m);
    let extent = nexrad::model::geo::GeoExtent {
        min: nexrad::model::geo::GeoPoint { latitude: ext[1], longitude: ext[0] },
        max: nexrad::model::geo::GeoPoint { latitude: ext[3], longitude: ext[2] },
    };
    ProductGrid::covering( &extent, res_lon, res_lat)
}

/// blend station grids onto the mosaic grid (see module doc). Cells that are not covered by any station get `no_echo`
pub fn blend (stations: &[Arc<StationGrid>], grid: &ProductGrid, blend_m: f64, no_echo: f32)->Vec<f32> {
    let blend_m = blend_m.max(1.0);
    let mut values = vec![ no_echo; grid.width * grid.height];

    parallel_rows( &mut values, grid.width, |row, out| {
        let mut cands: Vec<(f64,f64,f32)> = Vec::with_capacity( stations.len()); // (distance, range weight, value)
        for (col, v) in out.iter_mut().enumerate() {
            let p = grid.cell_center( row, col);
            cands.clear();
            for sg in stations {
                let d = sg.distance_m( p.latitude, p.longitude);
                if d < sg.max_range_m {
                    if let Some(x) = sg.value_at( p.latitude, p.longitude) {
                        cands.push( (d, ((sg.max_range_m - d) / blend_m).min(1.0), x));
                    }
                }
            }
            if cands.is_empty() { continue }

            let d_min = cands.iter().map( |c| c.0).fold( f64::MAX, f64::min);
            let (mut w_sum, mut x_sum) = (0.0, 0.0);
            for &(d, w_range, x) in &cands {
                let w = (1.0 - (d - d_min) / blend_m).max(0.0) * w_range;
                w_sum += w;
                x_sum += w * x as f64;
            }
            *v = if w_sum > 0.0 {
                (x_sum / w_sum) as f32
            } else { // all candidates at their range edge - use the nearest one
                cands.iter().min_by( |a,b| a.0.total_cmp( &b.0)).map( |c| c.2).unwrap_or( no_echo)
            };
        }
    });

    values
}

/// separable gaussian filter (edges are clamped)
pub fn smooth (values: &mut Vec<f32>, width: usize, height: usize, sigma_cells: f64) {
    if sigma_cells <= 0.0 || values.len() != width * height { return }
    let r = (3.0 * sigma_cells).ceil() as isize;
    let mut kernel: Vec<f32> = (-r..=r).map( |i| (-((i*i) as f64) / (2.0 * sigma_cells * sigma_cells)).exp() as f32).collect();
    let k_sum: f32 = kernel.iter().sum();
    kernel.iter_mut().for_each( |k| *k /= k_sum);

    let clamp = |i: isize, n: usize| i.clamp( 0, n as isize - 1) as usize;

    // horizontal
    let src = values.clone();
    parallel_rows( values, width, |row, out| {
        let src_row = &src[row*width..(row+1)*width];
        for (col, v) in out.iter_mut().enumerate() {
            *v = kernel.iter().enumerate().map( |(i,k)| k * src_row[ clamp( col as isize + i as isize - r, width)]).sum();
        }
    });

    // vertical
    let src = values.clone();
    parallel_rows( values, width, |row, out| {
        for (col, v) in out.iter_mut().enumerate() {
            *v = kernel.iter().enumerate().map( |(i,k)| k * src[ clamp( row as isize + i as isize - r, height) * width + col]).sum();
        }
    });
}

/// a reflectivity band of the mosaic: all areas with `lo <= dBZ < hi` (hi is None for the top band)
#[derive(Debug,Clone)]
pub struct ContourBand {
    pub lo: f32,
    pub hi: Option<f32>,
    pub polygons: Vec<Vec<Vec<[f64;2]>>>, // polygons -> rings (first is outer) -> [lon,lat] points
}

impl ContourBand {
    pub fn n_vertices (&self)->usize {
        self.polygons.iter().map( |p| p.iter().map( |r| r.len()).sum::<usize>()).sum()
    }
}

/// contour the mosaic into polygons for each band between `levels` (GDAL), simplify polygons and drop small ones
pub fn contour (values: &[f32], grid: &ProductGrid, levels: &[f32], simplify_m: f64, min_area_km2: f64)->Result<Vec<ContourBand>> {
    if values.len() != grid.width * grid.height { return Err( op_failed!("grid size mismatch")) }
    if levels.is_empty() { return Ok( Vec::new()) }

    let srs = SpatialRef::from_epsg( 4326)?;
    let mem = DriverManager::get_driver_by_name("MEM")?;
    let mut ds = mem.create_with_band_type::<f32,_>( "", grid.width, grid.height, 1)?;
    ds.set_geo_transform( &grid.geotransform())?;
    ds.set_spatial_ref( &srs)?;
    let mut band = ds.rasterband(1)?;
    band.write( (0,0), (grid.width, grid.height), &mut Buffer::new( (grid.width, grid.height), values.to_vec()))?;

    let mut vds = mem.create_vector_only( "")?;
    let mut layer = vds.create_layer( LayerOptions { name: "contour", srs: Some(&srs), ty: OGRwkbGeometryType::wkbMultiPolygon, options: None })?;
    layer.create_defn_fields( &[ ("lo", OGRFieldType::OFTReal), ("hi", OGRFieldType::OFTReal) ])?;

    // GDAL does not create the polygons above the highest level, hence we add one above the max value
    let top = values.iter().cloned().filter( |v| !v.is_nan()).fold( f32::MIN, f32::max).max( *levels.last().unwrap()) + 1.0;
    let level_list: Vec<String> = levels.iter().chain( std::iter::once( &top)).map( |l| l.to_string()).collect();
    let mut opts = CslStringList::new();
    opts.add_string( &format!("FIXED_LEVELS={}", level_list.join(",")))?;
    opts.add_string( "POLYGONIZE=YES")?;
    opts.add_string( "ELEV_FIELD_MIN=0")?;
    opts.add_string( "ELEV_FIELD_MAX=1")?;

    let err = unsafe { gdal_sys::GDALContourGenerateEx( band.c_rasterband(), layer.c_layer() as *mut _, opts.as_ptr(), None, null_mut()) };
    if err != gdal_sys::CPLErr::CE_None { return Err( op_failed!("contour generation failed")) }

    let tolerance = simplify_m / METERS_PER_DEG_LAT;
    let cos_lat = (grid.north - grid.height as f64 * grid.res_lat / 2.0).to_radians().cos();
    let min_area_deg2 = min_area_km2 * 1e6 / (METERS_PER_DEG_LAT * METERS_PER_DEG_LAT * cos_lat);
    let lowest = levels[0] - 1e-3;
    let highest = *levels.last().unwrap();

    let mut bands = Vec::new();
    for f in layer.features() {
        let lo = f.field_as_double(0)?.unwrap_or( f64::MIN) as f32;
        if lo < lowest { continue } // the area below the lowest level (no echo)
        let hi = f.field_as_double(1)?.map( |v| v as f32).filter( |v| *v <= highest + 1e-3 && *v > lo);
        let Some(geom) = f.geometry() else { continue };

        let geom = if tolerance > 0.0 { geom.simplify_preserve_topology( tolerance)? } else { geom.clone() };
        let mut polygons = Vec::new();
        collect_polygons( &geom, min_area_deg2, &mut polygons);
        if !polygons.is_empty() {
            bands.push( ContourBand { lo: snap_level( lo, levels), hi: hi.map( |v| snap_level( v, levels)), polygons });
        }
    }
    bands.sort_by( |a,b| a.lo.total_cmp( &b.lo));
    Ok(bands)
}

/// GDAL reports band limits as f64 - map back to our (f32) levels
fn snap_level (v: f32, levels: &[f32])->f32 {
    levels.iter().cloned().find( |l| (l - v).abs() < 1e-3).unwrap_or(v)
}

fn collect_polygons (geom: &Geometry, min_area_deg2: f64, polygons: &mut Vec<Vec<Vec<[f64;2]>>>) {
    match geometry_type_flatten( geom.geometry_type()) {
        OGRwkbGeometryType::wkbPolygon => {
            let mut rings = Vec::new();
            for i in 0..geom.geometry_count() {
                let ring = geom.get_geometry(i);
                let mut pts = Vec::new();
                ring.get_points( &mut pts);
                let ring: Vec<[f64;2]> = pts.iter().map( |p| [p.0, p.1]).collect();
                if ring.len() < 4 || ring_area( &ring) < min_area_deg2 {
                    if i == 0 { return } else { continue } // drop small polygon (outer ring) or hole
                }
                rings.push( ring);
            }
            if !rings.is_empty() { polygons.push( rings) }
        }
        OGRwkbGeometryType::wkbMultiPolygon | OGRwkbGeometryType::wkbGeometryCollection => {
            for i in 0..geom.geometry_count() { collect_polygons( &geom.get_geometry(i), min_area_deg2, polygons) }
        }
        _ => {}
    }
}

/// area of a closed ring in deg² (shoelace)
fn ring_area (ring: &[[f64;2]])->f64 {
    let mut a = 0.0;
    for w in ring.windows(2) { a += w[0][0] * w[1][1] - w[1][0] * w[0][1] }
    (a / 2.0).abs()
}

/// run `f(row, row_values)` on all rows of a row-major grid in parallel
fn parallel_rows<T: Send, F> (values: &mut [T], width: usize, f: F) where F: Fn(usize, &mut [T]) + Sync {
    if width == 0 || values.is_empty() { return }
    let height = values.len() / width;
    let n_threads = thread::available_parallelism().map( |n| n.get()).unwrap_or(4).max(1).min( height.max(1));
    let rows_per_thread = height.div_ceil( n_threads);
    thread::scope( |scope| {
        for (chunk_idx, chunk) in values.chunks_mut( rows_per_thread * width).enumerate() {
            let f = &f;
            scope.spawn( move || {
                for (i, row_values) in chunk.chunks_mut( width).enumerate() {
                    f( chunk_idx * rows_per_thread + i, row_values);
                }
            });
        }
    });
}

/* #endregion mosaic computation */

/* #region mosaic products ********************************************************************************************/

/// a computed mosaic of a product
#[derive(Debug,Clone,PartialEq)]
pub struct NexradMosaic {
    pub product: NexradProduct,
    pub date: DateTime<Utc>,                       // newest scan time of the used station time steps
    pub stations: Vec<(String,DateTime<Utc>)>,     // used stations and their scan times
    pub path: PathBuf,                             // GeoJSON file
    pub n_polygons: usize,
    pub n_vertices: usize,
    pub compute_time: Duration,
}

impl NexradMosaic {
    pub fn file_name (&self)->String {
        self.path.file_name().and_then( |f| f.to_str()).unwrap_or("").to_string()
    }

    pub fn json_value (&self)->serde_json::Value {
        serde_json::json!({ "product": self.product.name(), "date": self.date.timestamp_millis(), "file": self.file_name(), "stations": stations_json( &self.stations) })
    }
}

/// a new mosaic and the ones of the same product that were dropped because of it. Mosaics are identified by product and
/// date, i.e. a new mosaic can replace a previous one with the same date (e.g. if a delayed time step of another station
/// came in) - this is not reported in `removed`
#[derive(Debug,Clone)]
pub struct MosaicUpdate {
    pub added: NexradMosaic,
    pub removed: Vec<NexradMosaic>,
}

impl MosaicUpdate {
    /// the added mosaic plus the dates of removed ones: `{product, date, file, stations:[{id,date},..], removed:[date,..]}`
    pub fn json_value (&self)->serde_json::Value {
        let mut v = self.added.json_value();
        v["removed"] = self.removed.iter().map( |m| m.date.timestamp_millis()).collect();
        v
    }
}

fn stations_json (stations: &[(String,DateTime<Utc>)])->serde_json::Value {
    stations.iter().map( |(id,t)| serde_json::json!({ "id": id, "date": t.timestamp_millis() })).collect()
}

/// mosaic file name, e.g. `cref_mosaic_20261118T231155Z.geojson`
pub fn mosaic_file_name (product: NexradProduct, date: &DateTime<Utc>)->String {
    format!("{}_mosaic_{}.geojson", product.name(), date_tag( date))
}

/// GeoJSON FeatureCollection with one MultiPolygon feature per band (properties: lo, hi \[dBZ\] and default fill color).
/// Coordinates are rounded to 4 decimals (~10m)
pub fn bands_geojson (bands: &[ContourBand], palette: &[[u8;4]])->serde_json::Value {
    let round = |v: f64| (v * 1e4).round() / 1e4;
    let features: Vec<serde_json::Value> = bands.iter().map( |b| {
        let c = palette[ dbz_to_code( b.lo) as usize];
        let coords: Vec<Vec<Vec<[f64;2]>>> = b.polygons.iter().map( |p| p.iter().map( |r| r.iter().map( |pt| [round(pt[0]), round(pt[1])]).collect()).collect()).collect();
        serde_json::json!({
            "type": "Feature",
            "properties": { "lo": b.lo, "hi": b.hi, "color": format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]) },
            "geometry": { "type": "MultiPolygon", "coordinates": coords }
        })
    }).collect();
    serde_json::json!({ "type": "FeatureCollection", "features": features })
}

/// the latest time step of each station for a product, if it is not older than max_age relative to the newest one
pub fn mosaic_inputs<'a> (latest: impl Iterator<Item=&'a NexradEntry>, max_age: Duration)->Vec<NexradEntry> {
    let latest: Vec<&NexradEntry> = latest.collect();
    let Some(newest) = latest.iter().map( |e| e.scan_time).max() else { return Vec::new() };
    let max_age = chrono::TimeDelta::from_std( max_age).unwrap_or( chrono::TimeDelta::MAX);
    latest.into_iter().filter( |e| newest - e.scan_time <= max_age).cloned().collect()
}

/// computes mosaics, keeping the reduced station grids of the last inputs (only new time steps have to be read)
pub struct MosaicBuilder {
    pub config: MosaicConfig,
    stations: Vec<Station>,
    grid: ProductGrid,
    palette: Vec<[u8;4]>,
    dir: PathBuf,
    cache: HashMap<PathBuf,Arc<StationGrid>>,
}

impl MosaicBuilder {
    pub fn new (config: MosaicConfig, stations: Vec<Station>, min_dbz: f32, cache_dir: &Path)->Result<Self> {
        let grid = mosaic_grid( &stations, config.resolution_m)?;
        Ok( MosaicBuilder { config, stations, grid, palette: reflectivity_palette( min_dbz), dir: cache_dir.join("mosaic"), cache: HashMap::new() })
    }

    pub fn grid (&self)->&ProductGrid { &self.grid }

    /// the directory of mosaic files
    pub fn dir (&self)->&Path { &self.dir }

    /// compute and store the mosaic of a product from the given station time steps (see [`mosaic_inputs`]). Time steps
    /// that can't be read (e.g. because they were just dropped) are reported and skipped. This is blocking and compute
    /// intensive
    pub fn build (&mut self, product: NexradProduct, entries: &[NexradEntry])->Result<NexradMosaic> {
        let t0 = Instant::now();
        let no_echo = self.config.no_echo_dbz();

        let mut grids = Vec::new();
        for e in entries {
            if let Some(sg) = self.cache.get( &e.path) { grids.push( sg.clone()); continue }
            let Some(station) = self.stations.iter().find( |s| s.id == e.station) else { continue };
            match StationGrid::from_entry( e, station, self.config.resolution_m, no_echo) {
                Ok(sg) => { let sg = Arc::new(sg); self.cache.insert( e.path.clone(), sg.clone()); grids.push( sg) }
                Err(err) => eprintln!("failed to read NEXRAD mosaic input {:?}: {err}", e.path)
            }
        }
        if grids.is_empty() { return Err( op_failed!("no mosaic inputs for {}", product.name())) }
        self.retain_cached( product, entries);

        let mut values = blend( &grids, &self.grid, self.config.blend_m, no_echo);
        smooth( &mut values, self.grid.width, self.grid.height, self.config.smooth_m / self.config.resolution_m);
        let bands = contour( &values, &self.grid, &self.config.levels, self.config.simplify_m, self.config.min_area_km2)?;

        let date = grids.iter().map( |g| g.scan_time).max().unwrap();
        let mut stations: Vec<(String,DateTime<Utc>)> = grids.iter().map( |g| (g.station.clone(), g.scan_time)).collect();
        stations.sort();

        let path = self.dir.join( mosaic_file_name( product, &date));
        let mut gj = bands_geojson( &bands, &self.palette);
        gj["stations"] = stations_json( &stations); // foreign member, used to restore mosaics from files (see load_cached_mosaics)
        write_atomic( &path, gj.to_string().as_bytes())?;

        Ok( NexradMosaic {
            product, date, stations, path,
            n_polygons: bands.iter().map( |b| b.polygons.len()).sum(),
            n_vertices: bands.iter().map( |b| b.n_vertices()).sum(),
            compute_time: t0.elapsed(),
        })
    }

    /// drop cached grids of this product that are not inputs anymore
    fn retain_cached (&mut self, product: NexradProduct, entries: &[NexradEntry]) {
        let dir_name = product.name();
        self.cache.retain( |path, _| {
            let is_product = path.parent().and_then( |p| p.file_name()).map( |n| n == dir_name).unwrap_or(false);
            !is_product || entries.iter().any( |e| e.path == *path)
        });
    }
}

/// restore the mosaics of the given products from the GeoJSON files in `dir` (e.g. after restart), oldest first. Only the
/// `max_mosaics` newest ones per product are kept, older or unreadable files are removed
pub fn load_cached_mosaics (dir: &Path, products: &[NexradProduct], max_mosaics: usize)->Vec<NexradMosaic> {
    let mut mosaics = Vec::new();
    let Ok(rd) = std::fs::read_dir( dir) else { return mosaics };
    let mut paths: Vec<PathBuf> = rd.flatten().map( |de| de.path()).filter( |p| p.is_file()).collect();
    paths.sort(); // date tags sort chronologically

    for product in products {
        let prefix = format!("{}_mosaic_", product.name());
        let mut pms: Vec<NexradMosaic> = paths.iter().filter_map( |path| {
            let date = path.file_name()?.to_str()?.strip_prefix( &prefix)?.strip_suffix(".geojson").and_then( parse_date_tag)?;
            match read_mosaic( path, *product, date) {
                Some(m) => Some(m),
                None => { let _ = std::fs::remove_file( path); None }
            }
        }).collect();
        let n_drop = pms.len().saturating_sub( max_mosaics);
        for m in pms.drain( ..n_drop) { let _ = std::fs::remove_file( &m.path); }
        mosaics.extend( pms);
    }

    // remove files that are not mosaics of configured products (e.g. tmp files)
    for path in &paths {
        if !mosaics.iter().any( |m| m.path == *path) { let _ = std::fs::remove_file( path); }
    }
    mosaics
}

fn read_mosaic (path: &Path, product: NexradProduct, date: DateTime<Utc>)->Option<NexradMosaic> {
    let gj: serde_json::Value = serde_json::from_slice( &std::fs::read( path).ok()?).ok()?;
    let stations: Vec<(String,DateTime<Utc>)> = gj["stations"].as_array()?.iter().filter_map( |s| {
        Some( (s["id"].as_str()?.to_string(), DateTime::from_timestamp_millis( s["date"].as_i64()?)?))
    }).collect();
    let polygons: Vec<&serde_json::Value> = gj["features"].as_array()?.iter().filter_map( |f| f["geometry"]["coordinates"].as_array()).flatten().collect();
    let n_vertices = polygons.iter().filter_map( |p| p.as_array()).flatten().filter_map( |r| r.as_array()).map( |r| r.len()).sum();

    Some( NexradMosaic { product, date, stations, path: path.to_path_buf(), n_polygons: polygons.len(), n_vertices, compute_time: Duration::ZERO })
}

/// remove the GeoJSON files of mosaics
pub fn remove_mosaic_files (mosaics: &[NexradMosaic]) {
    for m in mosaics {
        if let Err(err) = std::fs::remove_file( &m.path) {
            if err.kind() != std::io::ErrorKind::NotFound { eprintln!("failed to remove {:?}: {err}", m.path) }
        }
    }
}

fn write_atomic (path: &Path, data: &[u8])->Result<()> {
    if let Some(dir) = path.parent() { std::fs::create_dir_all( dir)?; }
    let tmp_path = path.with_extension("tmp");
    std::fs::write( &tmp_path, data)?;
    std::fs::rename( &tmp_path, path)?;
    Ok(())
}

/* #endregion mosaic products */
