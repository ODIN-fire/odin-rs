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

//! local Cesium terrain from a (geographic) DEM VRT such as the ones created by `odin_dem::build_dem_vrt`.
//!
//! Tiles use the Cesium [heightmap-1.0](https://github.com/CesiumGS/cesium/wiki/heightmap-1.0-terrain-format) format
//! (65x65 u16 samples with `h = v/5 - 1000`, followed by a child tile mask byte) in a geographic (EPSG:4326) TMS tiling
//! scheme with 2x1 tiles at level 0. Tiles are computed on demand from the VRT (using source overviews for low levels),
//! converted from orthometric to ellipsoidal heights with a geoid undulation grid (e.g. PROJ's EGM2008
//! `us_nga_egm08_25.tif`) and stored gzipped in a disk cache, from where they are served with `Content-Encoding: gzip`.
//!
//! The `layer.json` lists tile availability per level, which is required by `Cesium.sampleTerrainMostDetailed`. Levels
//! up to `global_levels` are available world-wide (outside of the DEM tiles only contain the geoid surface), above that
//! availability is computed from the extents and resolutions of the VRT sources (following nested VRTs).

use std::{fs, io::Write, path::{Path, PathBuf}, sync::{atomic::{AtomicU64, AtomicUsize, Ordering}, Arc, Mutex, OnceLock}, time::{SystemTime, UNIX_EPOCH}};
use anyhow::{anyhow, Context, Result};
use flate2::{write::GzEncoder, Compression};
use serde::Deserialize;
use serde_json::json;

use odin_common::fs::EnvPathBuf;
use odin_gdal::{gdal::raster::ResampleAlg, Buffer, Dataset};

/// number of samples per tile row/column (tiles share edge samples)
pub const TILE_SIZE: usize = 65;
const HEIGHT_SCALE: f64 = 5.0;     // heightmap-1.0: h = v/5 - 1000
const HEIGHT_OFFSET: f64 = 1000.0;
const FORMAT_VERSION: u32 = 1;     // bump if tile computation changes (invalidates caches)

/// configuration of local terrain (normally loaded from `$ODIN_ROOT/configs/odin_cesium/terrain.ron`)
#[derive(Debug,Clone,Deserialize)]
pub struct TerrainConfig {
    /// the DEM VRT (geographic, heights in meters above MSL). If not set this is the `vrt_path` of the odin_dem
    /// `dem.ron` config
    #[serde(default)]
    pub vrt_path: Option<EnvPathBuf>,

    /// geoid undulation grid to convert orthometric (MSL) DEM heights into ellipsoidal heights as used by Cesium.
    /// Normally this is the EGM2008 grid of PROJ (`<proj-data>/us_nga_egm08_25.tif`). If not set heights are not
    /// converted, i.e. terrain will be 20-40m too high in CONUS with respect to ellipsoidal heights (e.g. for aircraft)
    #[serde(default)]
    pub geoid_path: Option<EnvPathBuf>,

    /// levels `0..=global_levels` are available world-wide (geoid surface outside of DEM)
    #[serde(default="default_global_levels")]
    pub global_levels: u32,

    /// max level. If not set this is computed from the source resolutions (15 for 1/3 arcsec 3DEP)
    #[serde(default)]
    pub max_level: Option<u32>,

    /// pre-compute missing tiles up to this level in a background thread when the service starts (low level tiles cover
    /// many DEM source files and can take seconds to compute). None disables seeding
    #[serde(default="default_seed_level")]
    pub seed_level: Option<u32>,

    /// tile cache directory (default is `$ODIN_ROOT/cache/odin_cesium/terrain`). Tiles are stored in a
    /// `<version>` subdirectory, the version changes if VRT files are modified
    #[serde(default)]
    pub cache_dir: Option<EnvPathBuf>,
}

fn default_global_levels()->u32 { 5 }
fn default_seed_level()->Option<u32> { Some(6) }

/* #region tiling scheme *********************************************************************************************/

/// geographic bounds of a TMS tile in degrees: (west, south, east, north)
pub fn tile_bounds (z: u32, x: u32, y: u32)->(f64,f64,f64,f64) {
    let size = 180.0 / (1u64 << z) as f64;
    let west = -180.0 + x as f64 * size;
    let south = -90.0 + y as f64 * size;
    (west, south, west + size, south + size)
}

pub fn n_tiles_x (z: u32)->u32 { 2 << z }
pub fn n_tiles_y (z: u32)->u32 { 1 << z }

/// sample spacing in degrees at level z
pub fn sample_spacing (z: u32)->f64 { 180.0 / (1u64 << z) as f64 / (TILE_SIZE - 1) as f64 }

/// the level at which the sample spacing becomes smaller or equal than the given resolution (degrees)
pub fn level_for_resolution (res: f64)->u32 {
    let mut z = 0;
    while sample_spacing(z) > res * 1.0001 && z < 24 { z += 1 }
    z
}

/// inclusive TMS tile range (x0,y0,x1,y1)
#[derive(Debug,Clone,Copy,PartialEq)]
pub struct TileRange { pub x0: u32, pub y0: u32, pub x1: u32, pub y1: u32 }

impl TileRange {
    /// tile range covering geographic bounds (tiles that only touch the bounds are excluded)
    pub fn for_bounds (z: u32, west: f64, south: f64, east: f64, north: f64)->Option<TileRange> {
        let size = 180.0 / (1u64 << z) as f64;
        let eps = size * 1e-6;
        let clamp_x = |v: f64| (v.max(0.0) as u32).min( n_tiles_x(z) - 1);
        let clamp_y = |v: f64| (v.max(0.0) as u32).min( n_tiles_y(z) - 1);
        let x0 = clamp_x( ((west + 180.0 + eps) / size).floor());
        let x1 = clamp_x( ((east + 180.0 - eps) / size).floor());
        let y0 = clamp_y( ((south + 90.0 + eps) / size).floor());
        let y1 = clamp_y( ((north + 90.0 - eps) / size).floor());
        if x1 >= x0 && y1 >= y0 && east > west && north > south { Some( TileRange{x0,y0,x1,y1}) } else { None }
    }

    pub fn contains (&self, x: u32, y: u32)->bool { x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1 }
}

/* #endregion tiling scheme */

/* #region VRT sources and availability ******************************************************************************/

/// geographic extent and resolution of a (leaf) VRT source
#[derive(Debug,Clone)]
pub struct SourceExtent {
    pub west: f64, pub south: f64, pub east: f64, pub north: f64,
    pub res: f64, // degrees (min of x/y resolution)
}

fn xml_attr (elem: &str, name: &str)->Option<f64> {
    let pat = format!("{name}=\"");
    let i = elem.find( &pat)? + pat.len();
    let j = elem[i..].find('"')? + i;
    elem[i..j].trim().parse().ok()
}

fn xml_elem<'a> (xml: &'a str, tag: &str)->Option<&'a str> {
    let i = xml.find( &format!("<{tag}"))?;
    let rest = &xml[i..];
    let j = rest.find('>')?;
    Some( &rest[..j])
}

fn xml_text<'a> (xml: &'a str, tag: &str)->Option<&'a str> {
    let i = xml.find( &format!("<{tag}"))?;
    let rest = &xml[i..];
    let j = rest.find('>')? + 1;
    let k = rest.find( &format!("</{tag}>"))?;
    Some( rest[j..k].trim())
}

fn xml_unescape (s: &str)->String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn mtime_secs (path: &Path)->u64 {
    fs::metadata( path).and_then( |m| m.modified()).ok()
        .and_then( |t| t.duration_since( UNIX_EPOCH).ok()).map( |d| d.as_secs()).unwrap_or(0)
}

/// collect geographic extents of all non-VRT sources referenced by a VRT (following nested VRTs). Also returns the max
/// modification time of all VRT files involved
pub fn vrt_source_extents (vrt_path: &Path)->Result<(Vec<SourceExtent>,u64)> {
    let mut extents = Vec::new();
    let mut mtime = 0;
    collect_extents( vrt_path, &mut extents, &mut mtime, 0)?;
    Ok( (extents, mtime) )
}

fn collect_extents (vrt_path: &Path, extents: &mut Vec<SourceExtent>, mtime: &mut u64, depth: usize)->Result<()> {
    if depth > 8 { return Err( anyhow!("VRT nesting too deep: {vrt_path:?}")) }
    let xml = fs::read_to_string( vrt_path).with_context( || format!("reading VRT {vrt_path:?}"))?;
    *mtime = (*mtime).max( mtime_secs( vrt_path));

    let gt: Vec<f64> = xml_text( &xml, "GeoTransform").ok_or_else( || anyhow!("no GeoTransform in {vrt_path:?}"))?
        .split(',').map( |s| s.trim().parse::<f64>()).collect::<std::result::Result<_,_>>()?;
    if gt.len() != 6 || gt[2] != 0.0 || gt[4] != 0.0 { return Err( anyhow!("unsupported GeoTransform in {vrt_path:?}")) }
    let vrt_dir = vrt_path.parent().unwrap_or( Path::new("."));

    // iterate over all <SourceFilename> elements, each one is followed by its <SrcRect> and <DstRect>
    let mut rest = xml.as_str();
    while let Some(i) = rest.find("<SourceFilename") {
        rest = &rest[i..];
        let end = rest[1..].find("<SourceFilename").map( |j| j+1).unwrap_or( rest.len());
        let src = &rest[..end];

        let fname_elem = xml_elem( src, "SourceFilename").unwrap_or("");
        let relative = xml_attr( fname_elem, "relativeToVRT").map( |v| v != 0.0).unwrap_or(false);
        let fname = xml_unescape( xml_text( src, "SourceFilename").unwrap_or(""));
        let path = if relative { vrt_dir.join( &fname) } else { PathBuf::from( &fname) };

        if fname.to_lowercase().ends_with(".vrt") {
            collect_extents( &path, extents, mtime, depth+1)?;
        } else if let Some(dst) = xml_elem( src, "DstRect") {
            let (x, y) = (xml_attr( dst, "xOff").unwrap_or(0.0), xml_attr( dst, "yOff").unwrap_or(0.0));
            let (w, h) = (xml_attr( dst, "xSize").unwrap_or(0.0), xml_attr( dst, "ySize").unwrap_or(0.0));
            let (sw, sh) = xml_elem( src, "SrcRect")
                .map( |r| (xml_attr( r, "xSize").unwrap_or(w), xml_attr( r, "ySize").unwrap_or(h))).unwrap_or( (w,h));
            if w > 0.0 && h > 0.0 && sw > 0.0 && sh > 0.0 {
                let (lon0, lat0) = (gt[0] + x * gt[1], gt[3] + y * gt[5]);
                let (lon1, lat1) = (gt[0] + (x + w) * gt[1], gt[3] + (y + h) * gt[5]);
                let res = (gt[1] * w / sw).abs().min( (gt[5] * h / sh).abs());
                extents.push( SourceExtent{ west: lon0.min(lon1), east: lon0.max(lon1), south: lat0.min(lat1), north: lat0.max(lat1), res });
            }
        }
        rest = &rest[end..];
    }
    Ok(())
}

/// tile availability per level (index = level)
#[derive(Debug,Clone)]
pub struct Availability {
    pub levels: Vec<Vec<TileRange>>,
}

impl Availability {
    pub fn compute (extents: &[SourceExtent], global_levels: u32, max_level: u32)->Self {
        let mut levels = Vec::with_capacity( max_level as usize + 1);
        for z in 0..=max_level {
            if z <= global_levels {
                levels.push( vec![ TileRange{ x0: 0, y0: 0, x1: n_tiles_x(z)-1, y1: n_tiles_y(z)-1 } ]);
            } else {
                let ranges: Vec<TileRange> = extents.iter()
                    .filter( |e| level_for_resolution( e.res) >= z)
                    .filter_map( |e| TileRange::for_bounds( z, e.west, e.south, e.east, e.north))
                    .collect();
                levels.push( merge_ranges( ranges));
            }
        }
        Availability{ levels }
    }

    pub fn max_level (&self)->u32 { self.levels.len() as u32 - 1 }

    pub fn is_available (&self, z: u32, x: u32, y: u32)->bool {
        self.levels.get( z as usize).map( |rs| rs.iter().any( |r| r.contains(x,y))).unwrap_or(false)
    }

    /// heightmap-1.0 child mask: bit 0: SW, 1: SE, 2: NW, 3: NE
    pub fn child_mask (&self, z: u32, x: u32, y: u32)->u8 {
        let mut mask = 0;
        if self.is_available( z+1, 2*x, 2*y) { mask |= 1 }
        if self.is_available( z+1, 2*x+1, 2*y) { mask |= 2 }
        if self.is_available( z+1, 2*x, 2*y+1) { mask |= 4 }
        if self.is_available( z+1, 2*x+1, 2*y+1) { mask |= 8 }
        mask
    }

    pub fn to_json (&self)->serde_json::Value {
        serde_json::Value::Array( self.levels.iter().map( |rs| {
            serde_json::Value::Array( rs.iter().map( |r| json!({"startX": r.x0, "startY": r.y0, "endX": r.x1, "endY": r.y1})).collect())
        }).collect())
    }
}

/// compute a non-overlapping set of tile ranges covering the union of the given (possibly overlapping) ranges.
/// We use coordinate compression on the range boundaries, emit horizontal runs of covered cells per row and then merge
/// runs with identical x-extent of adjacent rows
fn merge_ranges (ranges: Vec<TileRange>)->Vec<TileRange> {
    if ranges.is_empty() { return ranges }
    let mut xs: Vec<u32> = ranges.iter().flat_map( |r| [r.x0, r.x1+1]).collect();
    let mut ys: Vec<u32> = ranges.iter().flat_map( |r| [r.y0, r.y1+1]).collect();
    xs.sort(); xs.dedup();
    ys.sort(); ys.dedup();
    let (nx, ny) = (xs.len() - 1, ys.len() - 1);

    let mut covered = vec![ false; nx * ny];
    for r in &ranges {
        let (i0, i1) = (xs.binary_search( &r.x0).unwrap(), xs.binary_search( &(r.x1+1)).unwrap());
        let (j0, j1) = (ys.binary_search( &r.y0).unwrap(), ys.binary_search( &(r.y1+1)).unwrap());
        for j in j0..j1 { for i in i0..i1 { covered[j*nx + i] = true } }
    }

    let mut runs: Vec<TileRange> = Vec::new();
    for j in 0..ny {
        let mut i = 0;
        while i < nx {
            if covered[j*nx + i] {
                let start = i;
                while i < nx && covered[j*nx + i] { i += 1 }
                runs.push( TileRange{ x0: xs[start], x1: xs[i]-1, y0: ys[j], y1: ys[j+1]-1 });
            } else {
                i += 1;
            }
        }
    }

    // merge vertically adjacent runs with the same x-extent
    runs.sort_by_key( |r| (r.x0, r.x1, r.y0));
    let mut result: Vec<TileRange> = Vec::new();
    for r in runs {
        match result.last_mut() {
            Some(m) if m.x0 == r.x0 && m.x1 == r.x1 && r.y0 == m.y1 + 1 => m.y1 = r.y1,
            _ => result.push( r)
        }
    }
    result.sort_by_key( |r| (r.y0, r.x0));
    result
}

/* #endregion VRT sources and availability */

/* #region raster sampling *******************************************************************************************/

/// bilinear interpolation in a row-major grid with NaN for missing values (missing neighbours are ignored)
fn bilinear (data: &[f32], w: usize, h: usize, u: f64, v: f64)->f32 {
    let u = u.clamp( 0.0, (w - 1) as f64);
    let v = v.clamp( 0.0, (h - 1) as f64);
    let (i0, j0) = (u.floor() as usize, v.floor() as usize);
    let (i1, j1) = ((i0 + 1).min( w - 1), (j0 + 1).min( h - 1));
    let (fu, fv) = (u - i0 as f64, v - j0 as f64);
    let mut sum = 0.0;
    let mut wsum = 0.0;
    for (i, j, wgt) in [(i0, j0, (1.0-fu)*(1.0-fv)), (i1, j0, fu*(1.0-fv)), (i0, j1, (1.0-fu)*fv), (i1, j1, fu*fv)] {
        let d = data[j*w + i];
        if !d.is_nan() && wgt > 0.0 { sum += wgt * d as f64; wsum += wgt; }
    }
    if wsum > 1e-9 { (sum / wsum) as f32 } else { f32::NAN }
}

/// sample a single band dataset on a regular geographic grid of `n x n` points with spacing `d` (degrees), starting
/// at (west,north). Returns row-major values (north to south), NaN for points outside of the dataset or NoData
pub fn sample_dataset (ds: &Dataset, west: f64, north: f64, d: f64, n: usize)->Result<Vec<f32>> {
    let gt = ds.geo_transform()?;
    let (ds_w, ds_h) = ds.raster_size();
    let band = ds.rasterband(1)?;
    let nodata = band.no_data_value();
    let mut result = vec![ f32::NAN; n*n];

    // continuous pixel coordinates of the sample grid
    let east = west + d * (n-1) as f64;
    let south = north - d * (n-1) as f64;
    let (px_min, px_max) = ((west - gt[0]) / gt[1], (east - gt[0]) / gt[1]);
    let (py_min, py_max) = ((north - gt[3]) / gt[5], (south - gt[3]) / gt[5]);

    // ratio of sample spacing and source pixel size - we read at about sample resolution (letting GDAL use overviews)
    let s = (d / gt[1].abs()).min( d / gt[5].abs()).max( 1.0);
    let x0 = (px_min - s).floor().max( 0.0) as isize;
    let x1 = (px_max + s).ceil().min( ds_w as f64) as isize;
    let y0 = (py_min - s).floor().max( 0.0) as isize;
    let y1 = (py_max + s).ceil().min( ds_h as f64) as isize;
    if x1 <= x0 || y1 <= y0 || px_max < -0.5 || px_min > ds_w as f64 + 0.5 || py_max < -0.5 || py_min > ds_h as f64 + 0.5 {
        return Ok( result) // no overlap
    }
    let (win_w, win_h) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let buf_w = ((win_w as f64 / s).round() as usize).clamp( 1, win_w);
    let buf_h = ((win_h as f64 / s).round() as usize).clamp( 1, win_h);
    let alg = if buf_w < win_w || buf_h < win_h { ResampleAlg::Average } else { ResampleAlg::NearestNeighbour };

    let buf: Buffer<f32> = band.read_as( (x0, y0), (win_w, win_h), (buf_w, buf_h), Some(alg))?;
    let mut data = buf.into_shape_and_vec().1;
    if let Some(nd) = nodata {
        let nd = nd as f32;
        for v in data.iter_mut() { if *v == nd || !v.is_finite() { *v = f32::NAN } }
    }
    let (sx, sy) = (buf_w as f64 / win_w as f64, buf_h as f64 / win_h as f64); // buffer pixels per source pixel

    for j in 0..n {
        let py = (north - j as f64 * d - gt[3]) / gt[5];
        if py < -0.5 || py > ds_h as f64 + 0.5 { continue }
        let v = (py - y0 as f64) * sy - 0.5;
        for i in 0..n {
            let px = (west + i as f64 * d - gt[0]) / gt[1];
            if px < -0.5 || px > ds_w as f64 + 0.5 { continue }
            let u = (px - x0 as f64) * sx - 0.5;
            result[j*n + i] = bilinear( &data, buf_w, buf_h, u, v);
        }
    }
    Ok( result)
}

/// in-memory geoid undulation grid (global, geographic, pixel-is-point grid with longitude wrap-around)
pub struct GeoidGrid {
    data: Vec<f32>,
    nx: usize, ny: usize,
    lon0: f64, lat0: f64, // first grid point (west, north)
    d: f64,               // grid spacing in degrees
}

impl GeoidGrid {
    /// load a geoid grid, subsampling to a grid spacing of at least `min_spacing` degrees (to save memory - the full
    /// 2.5' EGM2008 grid has 150MB, 5' is accurate to a few cm)
    pub fn load (path: &Path, min_spacing: f64)->Result<Self> {
        let ds = Dataset::open( path).with_context( || format!("opening geoid grid {path:?}"))?;
        let gt = ds.geo_transform()?;
        let (w, h) = ds.raster_size();
        if (gt[1] + gt[5]).abs() > 1e-9 || gt[2] != 0.0 || gt[4] != 0.0 { return Err( anyhow!("unsupported geoid grid {path:?}")) }
        let res = gt[1];
        let stride = ((min_spacing / res).round() as usize).max(1);
        let band = ds.rasterband(1)?;
        let nx = (w + stride - 1) / stride;
        let ny = (h + stride - 1) / stride;
        let mut data = Vec::with_capacity( nx*ny);
        for j in (0..h).step_by( stride) {
            let row: Buffer<f32> = band.read_as( (0, j as isize), (w, 1), (w, 1), None)?;
            data.extend( row.data().iter().step_by( stride));
        }
        Ok( GeoidGrid{ data, nx, ny, lon0: gt[0] + res/2.0, lat0: gt[3] - res/2.0, d: res * stride as f64 })
    }

    /// geoid undulation at a geographic position (bilinear)
    pub fn undulation (&self, lon: f64, lat: f64)->f32 {
        let u = (lon - self.lon0) / self.d;
        let v = ((self.lat0 - lat) / self.d).clamp( 0.0, (self.ny - 1) as f64);
        let period = 360.0 / self.d; // grid columns per 360 deg
        let u = u.rem_euclid( period);
        let i0 = u.floor() as usize % self.nx;
        let i1 = if (i0 + 1) as f64 >= period.round() { 0 } else { (i0 + 1).min( self.nx - 1) };
        let j0 = v.floor() as usize;
        let j1 = (j0 + 1).min( self.ny - 1);
        let (fu, fv) = (u - u.floor(), v - j0 as f64);
        let g = |i: usize, j: usize| self.data[j*self.nx + i] as f64;
        ((1.0-fv) * ((1.0-fu) * g(i0,j0) + fu * g(i1,j0)) + fv * ((1.0-fu) * g(i0,j1) + fu * g(i1,j1))) as f32
    }
}

/* #endregion raster sampling */

/* #region TerrainTiles **********************************************************************************************/

/// on-demand heightmap-1.0 terrain tile generator with disk cache
pub struct TerrainTiles {
    pub vrt_path: PathBuf,
    pub geoid_path: Option<PathBuf>,
    pub version: String,
    pub cache_dir: PathBuf,
    pub availability: Availability,
    pub bounds: (f64,f64,f64,f64), // west, south, east, north of the DEM sources

    pub seed_level: Option<u32>,

    datasets: Mutex<Vec<Dataset>>, // pool of open VRT datasets (Dataset is Send but not Sync)
    geoid: OnceLock<Option<GeoidGrid>>,
}

impl TerrainTiles {
    pub fn new (config: &TerrainConfig)->Result<Self> {
        let vrt_path = match &config.vrt_path {
            Some(p) => p.0.clone(),
            None => {
                let dem_config: odin_dem::DemConfig = odin_dem::load_config("dem.ron").map_err( |e| anyhow!("no vrt_path in terrain config and no odin_dem config: {e}"))?;
                dem_config.vrt_path.0
            }
        };
        let geoid_path = config.geoid_path.as_ref().map( |p| p.0.clone());
        if let Some(p) = &geoid_path { if !p.is_file() { return Err( anyhow!("geoid grid not found: {p:?}")) } }

        let (extents, mtime) = vrt_source_extents( &vrt_path)?;
        if extents.is_empty() { return Err( anyhow!("no sources in VRT {vrt_path:?}")) }
        let max_level = config.max_level.unwrap_or_else( || extents.iter().map( |e| level_for_resolution( e.res)).max().unwrap_or(0));
        let availability = Availability::compute( &extents, config.global_levels, max_level.max( config.global_levels));
        let bounds = extents.iter().fold( (180.0f64, 90.0f64, -180.0f64, -90.0f64), |b, e| (b.0.min(e.west), b.1.min(e.south), b.2.max(e.east), b.3.max(e.north)));

        // the version reflects everything that changes tile contents
        let geoid_tag = geoid_path.as_ref().map( |p| format!("{}{}", p.display(), mtime_secs(p))).unwrap_or_default();
        let version = format!("{:x}", fnv1a( &format!("{FORMAT_VERSION}:{}:{mtime}:{geoid_tag}", vrt_path.display())));

        let cache_root = match &config.cache_dir {
            Some(p) => p.0.clone(),
            None => odin_build::cache_dir().join("odin_cesium").join("terrain")
        };
        let cache_dir = cache_root.join( &version);
        fs::create_dir_all( &cache_dir).with_context( || format!("creating terrain cache dir {cache_dir:?}"))?;

        Ok( TerrainTiles{ vrt_path, geoid_path, version, cache_dir, availability, bounds, seed_level: config.seed_level, datasets: Mutex::new( Vec::new()), geoid: OnceLock::new() })
    }

    /// the Cesium layer.json contents
    pub fn layer_json (&self)->String {
        json!({
            "tilejson": "2.1.0",
            "name": "odin",
            "description": format!("ODIN terrain from {}", self.vrt_path.file_name().map( |f| f.to_string_lossy()).unwrap_or_default()),
            "version": "1.0.0",
            "format": "heightmap-1.0",
            "scheme": "tms",
            "projection": "EPSG:4326",
            "tiles": [ format!("{{z}}/{{x}}/{{y}}.terrain?v={}", self.version) ],
            "bounds": [-180.0, -90.0, 180.0, 90.0],
            "minzoom": 0,
            "maxzoom": self.availability.max_level(),
            "available": self.availability.to_json(),
        }).to_string()
    }

    fn with_dataset<T> (&self, f: impl FnOnce(&Dataset)->Result<T>)->Result<T> {
        let ds = match self.datasets.lock().unwrap().pop() {
            Some(ds) => ds,
            None => Dataset::open( &self.vrt_path).with_context( || format!("opening DEM {:?}", self.vrt_path))?
        };
        let res = f( &ds);
        self.datasets.lock().unwrap().push( ds);
        res
    }

    fn geoid (&self)->Option<&GeoidGrid> {
        self.geoid.get_or_init( || {
            self.geoid_path.as_ref().and_then( |p| match GeoidGrid::load( p, 1.0/12.0) {
                Ok(g) => Some(g),
                Err(e) => { eprintln!("failed to load geoid grid: {e:#}"); None }
            })
        }).as_ref()
    }

    /// compute the uncompressed heightmap-1.0 tile data (65*65 u16 LE heights followed by the child mask byte)
    pub fn compute_tile (&self, z: u32, x: u32, y: u32)->Result<Vec<u8>> {
        if !self.availability.is_available( z, x, y) { return Err( anyhow!("tile {z}/{x}/{y} not available")) }
        let n = TILE_SIZE;
        let d = sample_spacing( z);
        let (west, south, east, north) = tile_bounds( z, x, y);

        let overlaps = west < self.bounds.2 && east > self.bounds.0 && south < self.bounds.3 && north > self.bounds.1;
        let heights = if overlaps { self.with_dataset( |ds| sample_dataset( ds, west, north, d, n))? } else { vec![f32::NAN; n*n] };
        let geoid = self.geoid();

        let mut buf = Vec::with_capacity( n*n*2 + 1);
        for j in 0..n {
            let lat = north - j as f64 * d;
            for i in 0..n {
                let h = heights[j*n + i];
                let h = if h.is_nan() { 0.0 } else { h as f64 }; // NoData is MSL (ocean)
                let n_geoid = geoid.map( |g| g.undulation( west + i as f64 * d, lat) as f64).unwrap_or(0.0);
                let v = ((h + n_geoid + HEIGHT_OFFSET) * HEIGHT_SCALE).round().clamp( 0.0, 65535.0) as u16;
                buf.extend_from_slice( &v.to_le_bytes());
            }
        }
        buf.push( self.availability.child_mask( z, x, y));
        Ok( buf)
    }

    /// all available tiles up to (including) `max_level`
    pub fn available_tiles (&self, max_level: u32)->Vec<(u32,u32,u32)> {
        let mut tiles = Vec::new();
        for z in 0..=max_level.min( self.availability.max_level()) {
            for r in &self.availability.levels[z as usize] {
                for x in r.x0..=r.x1 { for y in r.y0..=r.y1 { tiles.push( (z,x,y)) } }
            }
        }
        tiles
    }

    /// compute and cache all tiles up to (including) `max_level` that are not yet cached, using `n_threads`.
    /// Returns number of computed tiles and number of errors
    pub fn seed (&self, max_level: u32, n_threads: usize, progress: impl Fn(usize,usize) + Sync)->(usize,usize) {
        let work: Vec<_> = self.available_tiles( max_level).into_iter().filter( |(z,x,y)| !self.tile_path( *z, *x, *y).is_file()).collect();
        let next = AtomicUsize::new(0);
        let n_err = AtomicUsize::new(0);
        std::thread::scope( |s| {
            for _ in 0..n_threads.max(1) {
                s.spawn( || loop {
                    let i = next.fetch_add( 1, Ordering::Relaxed);
                    let Some((z,x,y)) = work.get(i).copied() else { break };
                    if let Err(e) = self.get_tile( z, x, y) {
                        eprintln!("terrain tile {z}/{x}/{y} failed: {e:#}");
                        n_err.fetch_add( 1, Ordering::Relaxed);
                    }
                    progress( i+1, work.len());
                });
            }
        });
        (work.len(), n_err.load( Ordering::Relaxed))
    }

    pub fn tile_path (&self, z: u32, x: u32, y: u32)->PathBuf {
        self.cache_dir.join( z.to_string()).join( x.to_string()).join( format!("{y}.terrain"))
    }

    /// gzipped tile data, either from the cache or computed (and then cached)
    pub fn get_tile (&self, z: u32, x: u32, y: u32)->Result<Vec<u8>> {
        let path = self.tile_path( z, x, y);
        if let Ok(data) = fs::read( &path) { return Ok(data) }

        let raw = self.compute_tile( z, x, y)?;
        let mut enc = GzEncoder::new( Vec::with_capacity( raw.len() / 2), Compression::default());
        enc.write_all( &raw)?;
        let data = enc.finish()?;

        if let Some(dir) = path.parent() { fs::create_dir_all( dir).with_context( || format!("creating {dir:?}"))?; }
        // tmp name has to be unique since the same tile might be computed concurrently
        static TMP_COUNT: AtomicU64 = AtomicU64::new(0);
        let tmp = path.with_extension( format!("terrain.{}.{}.part", std::process::id(), TMP_COUNT.fetch_add( 1, Ordering::Relaxed)));
        fs::write( &tmp, &data).with_context( || format!("writing {tmp:?}"))?;
        fs::rename( &tmp, &path).with_context( || format!("renaming {tmp:?}"))?;
        Ok( data)
    }
}

fn fnv1a (s: &str)->u64 {
    s.bytes().fold( 0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul( 0x100000001b3))
}

/// parse a `{z}/{x}/{y}.terrain` request path
pub fn parse_tile_path (path: &str)->Option<(u32,u32,u32)> {
    let mut it = path.trim_start_matches('/').split('/');
    let z = it.next()?.parse().ok()?;
    let x = it.next()?.parse().ok()?;
    let y = it.next()?.strip_suffix(".terrain")?.parse().ok()?;
    if it.next().is_some() { return None }
    Some( (z,x,y) )
}

/* #endregion TerrainTiles */

/* #region axum route ************************************************************************************************/

use axum::{http::{header, StatusCode}, response::{IntoResponse, Response}};

/// axum handler for `terrain/layer.json` and `terrain/{z}/{x}/{y}.terrain` requests
pub async fn terrain_handler (path: String, tiles: Arc<TerrainTiles>)->Response {
    if path == "layer.json" {
        return ([(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-cache")], tiles.layer_json()).into_response()
    }
    let Some((z,x,y)) = parse_tile_path( &path) else { return (StatusCode::NOT_FOUND, "invalid terrain tile path").into_response() };
    if !tiles.availability.is_available( z, x, y) { return (StatusCode::NOT_FOUND, "terrain tile not available").into_response() }

    match tokio::task::spawn_blocking( move || tiles.get_tile( z, x, y)).await {
        Ok(Ok(data)) => (
            [ (header::CONTENT_TYPE, "application/octet-stream"),
              (header::CONTENT_ENCODING, "gzip"),
              (header::CACHE_CONTROL, "public, max-age=604800") ], // URL includes the version
            data).into_response(),
        Ok(Err(e)) => (StatusCode::INTERNAL_SERVER_ERROR, format!("terrain tile {z}/{x}/{y} failed: {e:#}")).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("terrain tile task failed: {e}")).into_response(),
    }
}

/* #endregion axum route */
