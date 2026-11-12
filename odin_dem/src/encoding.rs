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

//! DEM source tile encodings and the functions to create encoded source tiles and VRTs from them.
//!
//! Supported encodings are
//!   - [`DemEncoding::U16`] (default): UInt16 with scale 0.1 and offset -500, i.e. 10cm resolution for heights
//!     in [-499.9, 6053.5]m, NoData 0. Requires a Float32 VRT that unscales the values (see [`build_dem_vrt`])
//!   - [`DemEncoding::I16`]: Int16 meters, NoData -32767 (the legacy ODIN encoding)
//!   - [`DemEncoding::F32`]: Float32 meters, NoData -999999 (the original 3DEP encoding)
//!
//! Encoded tiles are tiled GeoTIFFs (512x512 blocks, ZSTD compression with predictor) with internal overviews for
//! levels 4,8,16,32,64 (the 2x level is omitted since it accounts for ~2/3 of the overview size but is not needed for
//! normal access patterns)

use std::{fmt, fs, io::Write, path::{Path, PathBuf}, str::FromStr, time::Instant};
use serde::{Deserialize, Serialize};
use odin_gdal::{
    gdal::{config::{clear_thread_local_config_option, set_thread_local_config_option}, raster::GdalDataType},
    Buffer, Dataset, DriverManager, GdalType, RasterBand, RasterCreationOptions, SpatialRef
};
use crate::{errors::{op_failed, OdinDemError}, Result};

pub const U16_SCALE: f64 = 0.1;
pub const U16_OFFSET: f64 = -500.0;
pub const U16_NODATA: u16 = 0;
pub const I16_NODATA: i16 = -32767;
pub const F32_NODATA: f32 = -999999.0;

/// default overview levels for encoded DEM tiles
pub const DEFAULT_OVR_LEVELS: [i32;5] = [4, 8, 16, 32, 64];

/// the encoding of DEM source tiles (and the respective VRT)
#[derive(Debug,Clone,Copy,PartialEq,Eq,Hash,Serialize,Deserialize,Default)]
pub enum DemEncoding {
    /// UInt16 decimeters with offset -500m (heights = v*0.1 - 500), NoData 0
    #[default]
    U16,
    /// Int16 meters, NoData -32767
    I16,
    /// Float32 meters, NoData -999999
    F32,
}

impl DemEncoding {
    /// the data type of encoded tiles
    pub fn data_type (&self)->GdalDataType {
        match self { Self::U16 => GdalDataType::UInt16, Self::I16 => GdalDataType::Int16, Self::F32 => GdalDataType::Float32 }
    }

    /// the data type of the VRT band (heights in meters)
    pub fn vrt_data_type (&self)->GdalDataType {
        match self { Self::I16 => GdalDataType::Int16, _ => GdalDataType::Float32 }
    }

    /// the NoData value of encoded tiles
    pub fn nodata (&self)->f64 {
        match self { Self::U16 => U16_NODATA as f64, Self::I16 => I16_NODATA as f64, Self::F32 => F32_NODATA as f64 }
    }

    /// the NoData value of the VRT band
    pub fn vrt_nodata (&self)->f64 {
        match self { Self::I16 => I16_NODATA as f64, _ => F32_NODATA as f64 }
    }

    /// (scale,offset) for encodings that require them
    pub fn scale_offset (&self)->Option<(f64,f64)> {
        match self { Self::U16 => Some((U16_SCALE, U16_OFFSET)), _ => None }
    }

    /// max encoding error in meters (half the quantization step)
    pub fn max_error (&self)->f64 {
        match self { Self::U16 => U16_SCALE / 2.0, Self::I16 => 0.5, Self::F32 => 0.0 }
    }

    fn predictor (&self)->&'static str {
        match self { Self::F32 => "3", _ => "2" }
    }

    /// try to determine the encoding of a dataset band from its data type and scale/offset
    pub fn detect (band: &RasterBand)->Option<DemEncoding> {
        let (scale, offset) = (band.scale().unwrap_or(1.0), band.offset().unwrap_or(0.0));
        match band.band_type() {
            GdalDataType::UInt16 if (scale - U16_SCALE).abs() < 1e-9 && (offset - U16_OFFSET).abs() < 1e-9 => Some(Self::U16),
            GdalDataType::Int16 if scale == 1.0 && offset == 0.0 => Some(Self::I16),
            GdalDataType::Float32 if scale == 1.0 && offset == 0.0 => Some(Self::F32),
            _ => None
        }
    }
}

impl fmt::Display for DemEncoding {
    fn fmt (&self, f: &mut fmt::Formatter<'_>)->fmt::Result {
        f.write_str( match self { Self::U16 => "U16", Self::I16 => "I16", Self::F32 => "F32" })
    }
}

impl FromStr for DemEncoding {
    type Err = String;
    fn from_str (s: &str)->std::result::Result<Self,Self::Err> {
        match s.to_lowercase().as_str() {
            "u16" | "uint16" => Ok(Self::U16),
            "i16" | "int16" => Ok(Self::I16),
            "f32" | "float32" => Ok(Self::F32),
            _ => Err( format!("unknown DEM encoding '{s}' (expected u16, i16 or f32)"))
        }
    }
}

/* #region encoding tiles *************************************************************************************/

/// options for [`encode_dem_file`]
#[derive(Debug,Clone)]
pub struct EncodeOptions {
    pub compress: String,       // GTiff COMPRESS creation option
    pub block_size: usize,      // tile size
    pub ovr_levels: Vec<i32>,   // overview decimation factors (empty: no overviews)
    pub num_threads: usize,     // GDAL compression threads per file
}

impl Default for EncodeOptions {
    fn default()->Self {
        EncodeOptions { compress: "ZSTD".into(), block_size: 512, ovr_levels: DEFAULT_OVR_LEVELS.to_vec(), num_threads: 1 }
    }
}

/// statistics of an [`encode_dem_file`] call
#[derive(Debug,Clone,Default)]
pub struct EncodeStats {
    pub n_pixels: usize,
    pub n_nodata: usize,
    pub n_clamped: usize,   // values outside of the encoding range
    pub max_error: f64,     // max abs difference between source and encoded value [m] (not counting clamped values)
    pub src_bytes: u64,
    pub dst_bytes: u64,
    pub secs: f64,
}

/// convert a single DEM source file (any GDAL readable single band raster) into an encoded, tiled GeoTIFF with
/// overviews. The result is first written to `<dst>.part` and then renamed, i.e. existing `dst` files are only
/// replaced on success
pub fn encode_dem_file (src: impl AsRef<Path>, dst: impl AsRef<Path>, enc: DemEncoding, opts: &EncodeOptions)->Result<EncodeStats> {
    let (src, dst) = (src.as_ref(), dst.as_ref());
    let t0 = Instant::now();
    let src_ds = Dataset::open( src)?;
    if src_ds.raster_count() != 1 { return Err( op_failed( format!("{src:?} has {} bands (expected 1)", src_ds.raster_count()))) }

    let part = PathBuf::from( format!("{}.part", dst.display()));
    let mut stats = match enc {
        DemEncoding::U16 => {
            let (s, o) = (U16_SCALE, U16_OFFSET);
            encode_typed::<u16>( &src_ds, &part, enc, opts, U16_NODATA, move |v| {
                let c = ((v - o) / s).round();
                if c < 1.0 { (1, true, 0.0) } else if c > u16::MAX as f64 { (u16::MAX, true, 0.0) } else { (c as u16, false, (c * s + o - v).abs()) }
            })?
        }
        DemEncoding::I16 => {
            encode_typed::<i16>( &src_ds, &part, enc, opts, I16_NODATA, |v| {
                let c = v.round();
                if c < (I16_NODATA + 1) as f64 { (I16_NODATA + 1, true, 0.0) } else if c > i16::MAX as f64 { (i16::MAX, true, 0.0) } else { (c as i16, false, (c - v).abs()) }
            })?
        }
        DemEncoding::F32 => {
            encode_typed::<f32>( &src_ds, &part, enc, opts, F32_NODATA, |v| (v as f32, false, 0.0))?
        }
    };
    drop( src_ds);

    fs::rename( &part, dst)?;
    stats.src_bytes = fs::metadata( src).map( |m| m.len()).unwrap_or(0);
    stats.dst_bytes = fs::metadata( dst)?.len();
    stats.secs = t0.elapsed().as_secs_f64();
    Ok(stats)
}

fn encode_typed<T> (src_ds: &Dataset, path: &Path, enc: DemEncoding, opts: &EncodeOptions, enc_nodata: T, conv: impl Fn(f64)->(T,bool,f64))->Result<EncodeStats>
    where T: GdalType + Copy
{
    let src_band = src_ds.rasterband(1)?;
    let (w, h) = src_band.size();
    let src_nodata = src_band.no_data_value();
    // the source might be encoded itself
    let (src_scale, src_offset) = (src_band.scale().unwrap_or(1.0), src_band.offset().unwrap_or(0.0));

    let bs = opts.block_size.to_string();
    let mut co = RasterCreationOptions::new();
    co.add_name_value( "TILED", "YES")?;
    co.add_name_value( "BLOCKXSIZE", &bs)?;
    co.add_name_value( "BLOCKYSIZE", &bs)?;
    co.add_name_value( "COMPRESS", &opts.compress)?;
    co.add_name_value( "PREDICTOR", enc.predictor())?;
    co.add_name_value( "BIGTIFF", "IF_SAFER")?;
    co.add_name_value( "NUM_THREADS", &opts.num_threads.max(1).to_string())?;

    let driver = DriverManager::get_driver_by_name("GTiff")?;
    let mut ds = driver.create_with_band_type_with_options::<T,_>( path, w, h, 1, &co)?;
    ds.set_geo_transform( &src_ds.geo_transform()?)?;
    ds.set_spatial_ref( &src_ds.spatial_ref()?)?;

    let mut stats = EncodeStats { n_pixels: w*h, ..EncodeStats::default() };
    {
        let mut band = ds.rasterband(1)?;
        band.set_no_data_value( Some( enc.nodata()))?;
        if let Some((s,o)) = enc.scale_offset() {
            band.set_scale( s)?;
            band.set_offset( o)?;
        }

        // process in full-width strips of block rows so that we complete output tile rows
        let mut y = 0;
        while y < h {
            let n = opts.block_size.min( h - y);
            let buf: Buffer<f64> = src_band.read_as( (0, y as isize), (w, n), (w, n), None)?;
            let (_, data) = buf.into_shape_and_vec();
            let mut out: Vec<T> = Vec::with_capacity( data.len());
            for v in data {
                if Some(v) == src_nodata || !v.is_finite() {
                    out.push( enc_nodata);
                    stats.n_nodata += 1;
                } else {
                    let (c, clamped, err) = conv( v * src_scale + src_offset);
                    if clamped { stats.n_clamped += 1 } else if err > stats.max_error { stats.max_error = err }
                    out.push( c);
                }
            }
            band.write( (0, y as isize), (w, n), &mut Buffer::new( (w, n), out))?;
            y += n;
        }
    }

    let levels: Vec<i32> = opts.ovr_levels.iter().copied().filter( |l| *l > 1 && w.min(h) / *l as usize >= 16).collect();
    if !levels.is_empty() {
        ds.flush_cache()?;
        let n_threads = opts.num_threads.max(1).to_string();
        let ovr_opts = [("COMPRESS_OVERVIEW", opts.compress.as_str()), ("PREDICTOR_OVERVIEW", enc.predictor()),
                        ("GDAL_TIFF_OVR_BLOCKSIZE", bs.as_str()), ("GDAL_NUM_THREADS", n_threads.as_str())];
        for (k,v) in ovr_opts { set_thread_local_config_option( k, v)?; }
        let res = ds.build_overviews( "AVERAGE", &levels, &[]);
        for (k,_) in ovr_opts { clear_thread_local_config_option( k)?; }
        res?;
    }
    ds.flush_cache()?;
    Ok(stats)
}

/* #endregion encoding tiles */

/* #region building VRTs **************************************************************************************/

/// the relevant properties of a VRT source file
#[derive(Debug,Clone)]
pub struct DemSourceInfo {
    pub path: PathBuf,
    pub width: usize,
    pub height: usize,
    pub geo_transform: [f64;6],
    pub block_size: (usize,usize),
    pub data_type: GdalDataType,
    pub nodata: Option<f64>,
    pub scale: Option<f64>,
    pub offset: Option<f64>,
    pub n_overviews: i32,
    pub encoding: Option<DemEncoding>,
    pub srs: SpatialRef,
}

impl DemSourceInfo {
    pub fn read (path: impl AsRef<Path>)->Result<Self> {
        let path = path.as_ref();
        let ds = Dataset::open( path)?;
        if ds.raster_count() != 1 { return Err( op_failed( format!("{path:?} has {} bands (expected 1)", ds.raster_count()))) }
        let band = ds.rasterband(1)?;
        let (width, height) = band.size();
        let geo_transform = ds.geo_transform()?;
        if geo_transform[2] != 0.0 || geo_transform[4] != 0.0 || geo_transform[5] >= 0.0 {
            return Err( op_failed( format!("{path:?} is not north-up")))
        }
        Ok( DemSourceInfo {
            path: path.to_path_buf(), width, height, geo_transform,
            block_size: band.block_size(),
            data_type: band.band_type(),
            nodata: band.no_data_value(),
            scale: band.scale().filter( |s| *s != 1.0),
            offset: band.offset().filter( |o| *o != 0.0),
            n_overviews: band.overview_count()?,
            encoding: DemEncoding::detect( &band),
            srs: ds.spatial_ref()?,
        })
    }

    fn res (&self)->(f64,f64) { (self.geo_transform[1], -self.geo_transform[5]) }
}

/// statistics/diagnostics of a [`build_dem_vrt`] call
#[derive(Debug,Clone,Default)]
pub struct VrtStats {
    pub n_sources: usize,
    pub width: usize,
    pub height: usize,
    pub res: (f64,f64),
    pub bounds: [f64;4],        // west, south, east, north (in source SRS)
    pub files: Vec<PathBuf>,    // created VRT files (the first one is the VRT to use)
    pub warnings: Vec<String>,
}

/// relative pixel size differences below this are considered to be the same resolution (3DEP tiles differ in the 10th digit)
const SAME_RES_EPS: f64 = 1e-6;
/// fractional pixel offsets below this are rounded
const PIXEL_EPS: f64 = 1e-3;

/// the VRT grid of a set of sources
struct VrtGrid { width: usize, height: usize, west: f64, north: f64, res_x: f64, res_y: f64, header: String }

/// create a VRT for the given DEM source files, which have to be single band rasters with the same (north-up) SRS and
/// the given encoding. The VRT has the resolution of the finest source. Sources are added in the given order, i.e.
/// later sources take precedence where they overlap (NoData pixels never overwrite).
///
/// For I16 and F32 encodings this is a single mosaic VRT (Int16 or Float32 meters).
///
/// For U16 the VRT band is Float32 meters, i.e. clients do not need to know about the encoding. Since GDAL does not use
/// source overviews for resampled reads of VRT sources with scale/offset (which made coarse reads >100x slower), this
/// uses three VRT files:
///   - `<name>.u16.vrt`: the UInt16 mosaic of the sources (NoData 0, raw values)
///   - `<name>.u16x.vrt`: derived Float32 band of the mosaic, multiplied by the scale (pixel function `mul`)
///   - `<name>.vrt`: derived Float32 band adding the offset (pixel function `sum`) - this is the VRT to use
/// Derived bands do read their sources at the requested resolution, i.e. source overviews are used. We do not use the
/// `scale` pixel function since that requires band `Offset`/`Scale` metadata, which would be propagated into (already
/// unscaled) results of `gdalwarp` and then applied again by clients such as QGIS
///
/// This returns warnings for sources that will be slow to access through the VRT (no overviews, strip layouts) or
/// are not aligned with the VRT grid
pub fn build_dem_vrt (sources: &[PathBuf], vrt_path: impl AsRef<Path>, enc: DemEncoding)->Result<VrtStats> {
    let vrt_path = vrt_path.as_ref();
    if sources.is_empty() { return Err( op_failed("no VRT sources")) }

    let infos: Vec<DemSourceInfo> = sources.iter().map( DemSourceInfo::read).collect::<Result<_>>()?;
    let mut warnings = Vec::new();
    for info in &infos {
        if info.srs != infos[0].srs { return Err( op_failed( format!("{:?} has a different SRS than {:?}", info.path, infos[0].path))) }
        match info.encoding {
            Some(e) if e == enc => {}
            Some(e) => return Err( op_failed( format!("{:?} has encoding {e} (VRT is {enc})", info.path))),
            None => return Err( op_failed( format!("{:?} has an unsupported encoding ({:?}, scale {:?}, offset {:?})",
                                                  info.path, info.data_type, info.scale, info.offset)))
        }
        if info.n_overviews == 0 { warnings.push( format!("{:?} has no overviews", info.path)) }
        if info.block_size.1 < 16 { warnings.push( format!("{:?} is not tiled (blocks {:?})", info.path, info.block_size)) }
    }

    let vrt_dir = vrt_path.parent().filter( |p| !p.as_os_str().is_empty()).unwrap_or( Path::new("."));
    fs::create_dir_all( vrt_dir)?;
    let vrt_dir = fs::canonicalize( vrt_dir)?;
    let grid = vrt_grid( &infos)?;
    let mut files = Vec::new();

    match enc {
        DemEncoding::I16 | DemEncoding::F32 => {
            let xml = mosaic_xml( &grid, &infos, &vrt_dir, enc.vrt_data_type(), enc.vrt_nodata(), enc, "", &mut warnings)?;
            write_vrt( vrt_path, &xml)?;
            files.push( vrt_path.to_path_buf());
        }
        DemEncoding::U16 => {
            let stem = vrt_path.file_stem().map( |s| s.to_string_lossy().to_string()).unwrap_or( "dem".into());
            let mosaic_path = vrt_dir.join( format!("{stem}.u16.vrt"));
            let scaled_path = vrt_dir.join( format!("{stem}.u16x.vrt"));
            let fname = |p: &Path| p.file_name().unwrap().to_string_lossy().to_string();

            // the raw mosaic carries Offset/Scale metadata for documentation (GDAL does not apply it)
            let md = format!("    <Offset>{U16_OFFSET}</Offset>\n    <Scale>{U16_SCALE}</Scale>\n");
            let xml = mosaic_xml( &grid, &infos, &vrt_dir, GdalDataType::UInt16, U16_NODATA as f64, enc, &md, &mut warnings)?;
            write_vrt( &mosaic_path, &xml)?;

            let xml = derived_xml( &grid, "mul", &format!("k=\"{U16_SCALE}\""), &fname( &mosaic_path), U16_NODATA as f64, None);
            write_vrt( &scaled_path, &xml)?;

            let xml = derived_xml( &grid, "sum", &format!("k=\"{U16_OFFSET}\""), &fname( &scaled_path), F32_NODATA as f64, Some(enc));
            write_vrt( vrt_path, &xml)?;

            files.extend( [vrt_path.to_path_buf(), scaled_path, mosaic_path]);
        }
    }

    let (east, south) = (grid.west + grid.width as f64 * grid.res_x, grid.north - grid.height as f64 * grid.res_y);
    Ok( VrtStats { n_sources: infos.len(), width: grid.width, height: grid.height, res: (grid.res_x, grid.res_y),
                   bounds: [grid.west, south, east, grid.north], files, warnings })
}

fn vrt_grid (infos: &[DemSourceInfo])->Result<VrtGrid> {
    let res_x = infos.iter().map( |i| i.res().0).fold( f64::MAX, f64::min);
    let res_y = infos.iter().map( |i| i.res().1).fold( f64::MAX, f64::min);
    let west = infos.iter().map( |i| i.geo_transform[0]).fold( f64::MAX, f64::min);
    let north = infos.iter().map( |i| i.geo_transform[3]).fold( f64::MIN, f64::max);
    let east = infos.iter().map( |i| i.geo_transform[0] + i.width as f64 * i.res().0).fold( f64::MIN, f64::max);
    let south = infos.iter().map( |i| i.geo_transform[3] - i.height as f64 * i.res().1).fold( f64::MAX, f64::min);
    let width = ((east - west) / res_x - PIXEL_EPS).ceil() as usize;
    let height = ((north - south) / res_y - PIXEL_EPS).ceil() as usize;

    let mut header = format!("<VRTDataset rasterXSize=\"{width}\" rasterYSize=\"{height}\">\n");
    header.push_str( &format!("  <SRS>{}</SRS>\n", xml_escape( &infos[0].srs.to_wkt()?)));
    header.push_str( &format!("  <GeoTransform>{west:.16e}, {res_x:.16e}, 0.0, {north:.16e}, 0.0, {:.16e}</GeoTransform>\n", -res_y));
    Ok( VrtGrid { width, height, west, north, res_x, res_y, header })
}

fn encoding_metadata (enc: DemEncoding)->String {
    format!("  <Metadata><MDI key=\"ODIN_DEM_ENCODING\">{enc}</MDI></Metadata>\n")
}

fn mosaic_xml (grid: &VrtGrid, infos: &[DemSourceInfo], vrt_dir: &Path, band_type: GdalDataType, nodata: f64, enc: DemEncoding,
               band_md: &str, warnings: &mut Vec<String>)->Result<String> {
    let mut xml = String::with_capacity( 1024 + infos.len() * 640);
    xml.push_str( &grid.header);
    xml.push_str( &encoding_metadata( enc));
    xml.push_str( &format!("  <VRTRasterBand dataType=\"{}\" band=\"1\">\n", band_type.name()));
    xml.push_str( &format!("    <NoDataValue>{nodata}</NoDataValue>\n"));
    xml.push_str( "    <ColorInterp>Gray</ColorInterp>\n");
    xml.push_str( band_md);

    for info in infos {
        let (rx, ry) = info.res();
        let x_off = snap( (info.geo_transform[0] - grid.west) / grid.res_x);
        let y_off = snap( (grid.north - info.geo_transform[3]) / grid.res_y);
        let same_res = ((rx - grid.res_x) / grid.res_x).abs() < SAME_RES_EPS && ((ry - grid.res_y) / grid.res_y).abs() < SAME_RES_EPS;
        let (x_size, y_size) = if same_res { (info.width as f64, info.height as f64) } else {
            (snap( info.width as f64 * rx / grid.res_x), snap( info.height as f64 * ry / grid.res_y))
        };
        if same_res && (x_off.fract() != 0.0 || y_off.fract() != 0.0) {
            warnings.push( format!("{:?} is not aligned with the VRT grid (offset {x_off},{y_off})", info.path))
        }

        let abs_path = fs::canonicalize( &info.path)?;
        let (rel, fname) = match abs_path.strip_prefix( vrt_dir) {
            Ok(p) => (1, p.to_path_buf()),
            Err(_) => (0, abs_path.clone())
        };
        let resampling = if same_res { "" } else { " resampling=\"bilinear\"" };
        xml.push_str( &format!("    <ComplexSource{resampling}>\n"));
        xml.push_str( &format!("      <SourceFilename relativeToVRT=\"{rel}\">{}</SourceFilename>\n", xml_escape( &fname.to_string_lossy())));
        xml.push_str( "      <SourceBand>1</SourceBand>\n");
        xml.push_str( &format!("      <SourceProperties RasterXSize=\"{}\" RasterYSize=\"{}\" DataType=\"{}\" BlockXSize=\"{}\" BlockYSize=\"{}\" />\n",
                               info.width, info.height, info.data_type.name(), info.block_size.0, info.block_size.1));
        xml.push_str( &format!("      <SrcRect xOff=\"0\" yOff=\"0\" xSize=\"{}\" ySize=\"{}\" />\n", info.width, info.height));
        xml.push_str( &format!("      <DstRect xOff=\"{x_off}\" yOff=\"{y_off}\" xSize=\"{x_size}\" ySize=\"{y_size}\" />\n"));
        if let Some(nd) = info.nodata { xml.push_str( &format!("      <NODATA>{nd}</NODATA>\n")); }
        xml.push_str( "    </ComplexSource>\n");
    }
    xml.push_str( "  </VRTRasterBand>\n</VRTDataset>\n");
    Ok(xml)
}

/// a Float32 VRT with a single derived band that applies a pixel function to a VRT in the same directory
fn derived_xml (grid: &VrtGrid, func: &str, args: &str, src_name: &str, src_nodata: f64, enc: Option<DemEncoding>)->String {
    let mut xml = grid.header.clone();
    if let Some(enc) = enc { xml.push_str( &encoding_metadata( enc)); }
    xml.push_str( &format!(r#"  <VRTRasterBand dataType="Float32" band="1" subClass="VRTDerivedRasterBand">
    <NoDataValue>{F32_NODATA}</NoDataValue>
    <ColorInterp>Gray</ColorInterp>
    <PixelFunctionType>{func}</PixelFunctionType>
    <PixelFunctionArguments {args} propagateNoData="true"/>
    <SourceTransferType>Float64</SourceTransferType>
    <ComplexSource>
      <SourceFilename relativeToVRT="1">{}</SourceFilename>
      <SourceBand>1</SourceBand>
      <SrcRect xOff="0" yOff="0" xSize="{w}" ySize="{h}" />
      <DstRect xOff="0" yOff="0" xSize="{w}" ySize="{h}" />
      <NODATA>{src_nodata}</NODATA>
    </ComplexSource>
  </VRTRasterBand>
</VRTDataset>
"#, xml_escape( src_name), w = grid.width, h = grid.height));
    xml
}

fn write_vrt (path: &Path, xml: &str)->Result<()> {
    let tmp = PathBuf::from( format!("{}.part", path.display()));
    fs::File::create( &tmp)?.write_all( xml.as_bytes())?;
    fs::rename( &tmp, path)?;
    Ok(())
}

/// round to the next integer if we are within PIXEL_EPS
fn snap (v: f64)->f64 {
    let r = v.round();
    if (v - r).abs() < PIXEL_EPS { r } else { v }
}

fn xml_escape (s: &str)->String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// collect `*.tif` / `*.tiff` files from the given paths, which can be files or directories. Files of directories are
/// sorted by name, AppleDouble (`._*`) and `*.part` files are skipped. The order of the paths is preserved
pub fn collect_dem_files (paths: &[PathBuf])->Result<Vec<PathBuf>> {
    let is_tif = |p: &Path| {
        let name = p.file_name().map( |n| n.to_string_lossy().to_string()).unwrap_or_default();
        !name.starts_with("._") && p.extension().map( |e| e.eq_ignore_ascii_case("tif") || e.eq_ignore_ascii_case("tiff")).unwrap_or(false)
    };
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            let mut dir_files: Vec<PathBuf> = fs::read_dir( path)?.filter_map( |e| e.ok().map( |e| e.path())).filter( |p| p.is_file() && is_tif(p)).collect();
            dir_files.sort();
            files.extend( dir_files);
        } else if path.is_file() {
            files.push( path.clone());
        } else {
            return Err( op_failed( format!("DEM source {path:?} not found")))
        }
    }
    Ok(files)
}

/* #endregion building VRTs */
