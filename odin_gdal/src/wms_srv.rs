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

//! a WMS server for (large, tiled) GeoTIFF datasets. GetMap images are computed on demand directly from the datasets,
//! i.e. we warp the requested area into a MEM dataset (nearest neighbour, preserving the data resolution) and colorize
//! it according to the requested WMS style:
//!  - "cmap" : categorical (8 or 16 bit integer) data with a value lookup table from a data source specific color map
//!  - "alpha": continuous (integer or float) data with a single (requested) color and the value mapped to the alpha channel
//! Nodata pixels are rendered with a (requested) nodata color. This does not require any derived data products.
//!
//! This module provides the generic parts (request parsing, layer configuration, rendering, caching and the request
//! handlers of `WmsService`). Data source crates (e.g. odin_landfire) provide the configuration, color map readers and
//! the routes for their `odin_server::spa::SpaService` implementations

use std::{collections::HashMap, io::Cursor, path::{Path,PathBuf}, sync::{Arc, atomic::{AtomicUsize, Ordering}}};
use axum::{http::{HeaderMap, StatusCode, Uri, header}, response::{IntoResponse, Response}};
use gdal::{Dataset, raster::GdalDataType, spatial_ref::{AxisMappingStrategy, CoordTransform, SpatialRef}};
use image::{ImageEncoder, codecs::png::{CompressionType, FilterType, PngEncoder}, ExtendedColorType};
use serde::Deserialize;
use tokio::sync::Semaphore;

use odin_common::{BoundingBox, fs::{self, EnvPathBuf}, strings::parse_array};

use crate::{
    get_rasterband_as_vec,
    cmap::{CmapEntry, CmapReader, ValueLut, colorize_alpha, json_legend, parse_rgb, parse_rgba},
    errors::{Result, misc_error},
    geods::{GeoDsImgType, GeoDsSrs},
    warp::{ResampleAlg, SimpleWarpBuilder},
};

/// default color for alpha encoded layers if the request does not specify one (vendor specific COLOR parameter)
const DEFAULT_ALPHA_RGB: [u8;3] = [255,255,255];

/// max width/height of GetMap images
const MAX_IMG_SIZE: u32 = 2048;

/// purge the image cache after this many new cache entries
const PURGE_INTERVAL: usize = 500;

/* #region WMS requests ***************************************************************************************/

/// the subset of WMS requests we support. This is what Cesium.WebMapServiceImageryProvider needs, plus GetCapabilities
#[derive(Debug)]
pub enum WmsRequest {
    GetCapabilities,
    GetMap(WmsGetMap),
    GetFeatureInfo(WmsGetFeatureInfo)
}

/// a parsed and normalized WMS GetFeatureInfo request
#[derive(Debug)]
pub struct WmsGetFeatureInfo {
    pub map: WmsGetMap, // the GetMap request the pixel position refers to
    pub query_layers: Vec<String>,
    pub i: u32, // pixel column (I in WMS 1.3.0, X in 1.1.1)
    pub j: u32, // pixel row (J in WMS 1.3.0, Y in 1.1.1)
    pub info_format: String,
}

impl WmsGetFeatureInfo {
    /// the position of the queried pixel center in x,y order (lon,lat for geographic CRS)
    pub fn position (&self)->(f64,f64) {
        let b = &self.map.bbox;
        let x = b.west + (self.i as f64 + 0.5) * (b.east - b.west) / self.map.width as f64;
        let y = b.north - (self.j as f64 + 0.5) * (b.north - b.south) / self.map.height as f64;
        (x, y)
    }
}

/// the GetFeatureInfo formats we support (GeoJSON FeatureCollections, as understood by Cesium)
pub const FEATURE_INFO_FORMATS: &[&str] = &["application/json", "application/geo+json"];

/// a parsed and normalized WMS GetMap request
#[derive(Debug)]
pub struct WmsGetMap {
    pub version: String,
    pub layers: Vec<String>,
    pub styles: Vec<String>,
    pub srs: GeoDsSrs,
    pub bbox: BoundingBox<f64>, // normalized to x/y order (lon,lat for geographic CRS), regardless of WMS version
    pub width: u32,
    pub height: u32,
    pub img_type: GeoDsImgType,
    pub transparent: bool,
    pub params: HashMap<String,String> // all request parameters with lowercase keys (for vendor specific parameters)
}

/// WMS ServiceException (we only support the XML exception format)
#[derive(Debug)]
pub struct WmsException {
    pub code: Option<&'static str>, // one of the WMS exception codes such as "LayerNotDefined"
    pub msg: String
}

impl WmsException {
    pub fn new (code: Option<&'static str>, msg: impl ToString)->Self { WmsException{ code, msg: msg.to_string() } }
    pub fn missing (param: &str)->Self { Self::new( Some("MissingParameterValue"), format!("missing or invalid {} parameter", param.to_uppercase())) }
}

impl IntoResponse for WmsException {
    fn into_response(self) -> axum::response::Response {
        let code_attr = if let Some(code) = self.code { format!(" code=\"{code}\"") } else { String::new() };
        let body = format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<ServiceExceptionReport version="1.3.0" xmlns="http://www.opengis.net/ogc">
  <ServiceException{code_attr}>{}</ServiceException>
</ServiceExceptionReport>
"#, xml_escape( &self.msg));
        (StatusCode::BAD_REQUEST, [(http::header::CONTENT_TYPE, "text/xml")], body).into_response()
    }
}

pub fn xml_escape (s: &str)->String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// parse WMS request parameters. Parameter names are case insensitive according to the WMS spec. We support both
/// WMS 1.1.1 (SRS) and 1.3.0 (CRS) - note that WMS 1.3.0 uses lat,lon axis order for EPSG:4326 BBOX values, which we
/// normalize into x,y order
pub fn parse_wms_request (query: &HashMap<String,String>)->std::result::Result<WmsRequest,WmsException> {
    let params: HashMap<String,String> = query.iter().map( |(k,v)| (k.to_lowercase(), v.clone())).collect();

    if let Some(service) = params.get("service") && !service.eq_ignore_ascii_case("WMS") {
        return Err( WmsException::new( Some("InvalidParameterValue"), "unsupported SERVICE"))
    }

    match params.get("request").map( |s| s.as_str()) {
        Some(r) if r.eq_ignore_ascii_case("GetCapabilities") => Ok( WmsRequest::GetCapabilities ),
        Some(r) if r.eq_ignore_ascii_case("GetMap") => parse_get_map( params).map( |gm| WmsRequest::GetMap(gm)),
        Some(r) if r.eq_ignore_ascii_case("GetFeatureInfo") => parse_get_feature_info( params).map( |gfi| WmsRequest::GetFeatureInfo(gfi)),
        Some(_) => Err( WmsException::new( Some("OperationNotSupported"), "unsupported REQUEST")),
        None => Err( WmsException::missing("request"))
    }
}

fn parse_get_map (params: HashMap<String,String>)->std::result::Result<WmsGetMap,WmsException> {
    let version = params.get("version").cloned().unwrap_or( "1.3.0".to_string());
    let is_v13 = version.starts_with("1.3");

    let srs_spec = if is_v13 { params.get("crs") } else { params.get("srs") }
        .or_else( || params.get("crs")).or_else( || params.get("srs")) // be lenient
        .ok_or( WmsException::missing( if is_v13 {"crs"} else {"srs"}))?;
    let srs = GeoDsSrs::from_srs_spec( srs_spec).ok_or( WmsException::new( Some("InvalidCRS"), format!("unsupported CRS {srs_spec}")))?;

    let bbox = params.get("bbox").and_then( |s| parse_array::<f64,4>( s, ',').ok()).ok_or( WmsException::missing("bbox"))?;
    let bbox = if is_v13 && matches!( srs, GeoDsSrs::WGS84) { // EPSG:4326 in WMS 1.3.0 is lat,lon
        BoundingBox::from_wsen( &[bbox[1], bbox[0], bbox[3], bbox[2]])
    } else {
        BoundingBox::from_wsen( &bbox)
    };
    if bbox.west >= bbox.east || bbox.south >= bbox.north {
        return Err( WmsException::new( Some("InvalidParameterValue"), "invalid BBOX"))
    }

    let width = params.get("width").and_then( |s| s.parse::<u32>().ok()).filter( |w| *w > 0).ok_or( WmsException::missing("width"))?;
    let height = params.get("height").and_then( |s| s.parse::<u32>().ok()).filter( |h| *h > 0).ok_or( WmsException::missing("height"))?;

    let format = params.get("format").ok_or( WmsException::missing("format"))?;
    let img_type = GeoDsImgType::for_mime_type( format).ok_or( WmsException::new( Some("InvalidFormat"), format!("unsupported FORMAT {format}")))?;

    let layers: Vec<String> = params.get("layers").map( |s| s.split(',').filter( |l| !l.is_empty()).map( |l| l.to_string()).collect()).unwrap_or_default();
    if layers.is_empty() { return Err( WmsException::missing("layers")) }
    let styles: Vec<String> = params.get("styles").map( |s| s.split(',').map( |l| l.to_string()).collect()).unwrap_or_default();

    let transparent = params.get("transparent").map( |s| s.eq_ignore_ascii_case("true")).unwrap_or(false);

    Ok( WmsGetMap{ version, layers, styles, srs, bbox, width, height, img_type, transparent, params } )
}

fn parse_get_feature_info (mut params: HashMap<String,String>)->std::result::Result<WmsGetFeatureInfo,WmsException> {
    let query_layers: Vec<String> = params.get("query_layers").map( |s| s.split(',').filter( |l| !l.is_empty()).map( |l| l.to_string()).collect()).unwrap_or_default();
    if query_layers.is_empty() { return Err( WmsException::missing("query_layers")) }

    // the embedded GetMap parameters - FORMAT is irrelevant here and LAYERS can default to QUERY_LAYERS
    if !params.contains_key("format") { params.insert( "format".into(), "image/png".into()); }
    if !params.contains_key("layers") { params.insert( "layers".into(), query_layers.join(",")); }

    let info_format = params.get("info_format").cloned().unwrap_or( FEATURE_INFO_FORMATS[0].to_string());
    if !FEATURE_INFO_FORMATS.contains( &info_format.as_str()) {
        return Err( WmsException::new( Some("InvalidFormat"), format!("unsupported INFO_FORMAT {info_format}")))
    }

    let is_v13 = params.get("version").map( |v| v.starts_with("1.3")).unwrap_or(true);
    let (ki, kj) = if is_v13 { ("i","j") } else { ("x","y") };
    let i = params.get(ki).or_else( || params.get("i")).or_else( || params.get("x")).and_then( |s| s.parse::<u32>().ok()).ok_or( WmsException::missing(ki))?;
    let j = params.get(kj).or_else( || params.get("j")).or_else( || params.get("y")).and_then( |s| s.parse::<u32>().ok()).ok_or( WmsException::missing(kj))?;

    let map = parse_get_map( params)?;
    if i >= map.width || j >= map.height {
        return Err( WmsException::new( Some("InvalidPoint"), "pixel position outside of image"))
    }

    Ok( WmsGetFeatureInfo { map, query_layers, i, j, info_format } )
}

/* #endregion WMS requests */

/* #region WMS layers *****************************************************************************************/

/// the static WMS configuration (normally part of the data source specific service config)
#[derive(Deserialize,Debug)]
pub struct WmsConfig {
    pub title: String,
    #[serde(default="default_max_cache")]
    pub max_cache: u64, // max size of GetMap image cache in bytes
    #[serde(default="default_max_renders")]
    pub max_renders: usize, // max number of concurrent GetMap image computations (each one opens the layer GeoTIFF)
    #[serde(default="default_max_downsample")]
    pub max_downsample: f64, // requests with pixels larger than this factor times the data cell size get transparent images
    pub layers: Vec<WmsLayerConfig>, // in order of capabilities
}

fn default_max_cache()->u64 { 1024*1024*1024 } // 1GB
fn default_max_renders()->usize { 4 }

// computation time is proportional to the source area, i.e. without overviews a 256x256 image for a 11deg (geographic
// level 4) tile already takes several seconds. Such requests are mostly for ancestor tiles Cesium uses as fallback
fn default_max_downsample()->f64 { 16.0 }

#[derive(Deserialize,Debug)]
pub struct WmsLayerConfig {
    pub name: String,
    pub title: String,
    pub descr: Option<String>,
    pub path: EnvPathBuf, // the (tiled) GeoTIFF
    #[serde(default)]
    pub nodata: Vec<f64>, // additional nodata values (the declared band nodata value is always included)
    pub styles: Vec<WmsStyleConfig>, // the first one is the default style
}

/// the supported WMS styles (colorizations). Each one can only be used once per layer since the style name is
/// derived from the type ("cmap" or "alpha")
#[derive(Deserialize,Debug)]
pub enum WmsStyleConfig {
    /// categorical (8 or 16 bit integer) data with a color map file that is read by a data source specific CmapReader
    Cmap(EnvPathBuf),
    /// continuous data mapped to the alpha channel of a single color. The value range [min,max] can be overridden by
    /// the vendor specific VRANGE=min,max GetMap parameter, the color is set with the vendor specific COLOR=RRGGBB parameter
    Alpha { min: f64, max: f64 },
}

/// the runtime representation of a configured WMS layer
pub struct WmsLayer {
    pub name: String,
    pub title: String,
    pub descr: Option<String>,
    pub path: PathBuf,
    pub nodata: Vec<f64>, // declared nodata, configured nodata values and nodata entries of color maps (NaN is always nodata)
    pub lut_base: Option<i32>, // smallest value of 8/16 bit integer data types (None if we can't use value lookup tables)
    pub geo_bounds: [f64;4], // west,south,east,north in degrees
    pub res_m: f64, // approximate data cell size in meters
    pub styles: Vec<WmsStyle>,
}

/// result of a GetFeatureInfo query for a single layer
#[derive(Debug)]
pub struct FeatureValue {
    pub value: f64,
    pub is_nodata: bool,
    pub name: Option<String>, // name of the color map entry for categorical data
}

pub struct WmsStyle {
    pub name: &'static str,
    pub title: &'static str,
    pub colorizer: Colorizer,
}

pub enum Colorizer {
    Lut { lut: ValueLut, entries: Vec<CmapEntry> },
    Alpha { min: f64, max: f64 },
}

impl WmsLayer {
    pub fn new (config: &WmsLayerConfig, read_cmap: Option<&CmapReader>)->Result<Self> {
        let ds = Dataset::open( &config.path)?;
        let geo_bounds = get_geo_bounds( &ds)?;
        let res_m = get_res_m( &ds, &geo_bounds)?;
        let band = ds.rasterband(1)?;
        let lut_base = get_lut_base( band.band_type());

        // note that nodata pixels outside of the dataset extent are set to the declared nodata value by the warp
        let mut nodata: Vec<f64> = config.nodata.clone();
        if let Some(v) = band.no_data_value() { add_unique( &mut nodata, v); }

        let mut styles: Vec<WmsStyle> = Vec::with_capacity( config.styles.len());
        for sc in &config.styles {
            let style = match sc {
                WmsStyleConfig::Cmap(path) => {
                    let base = lut_base.ok_or( misc_error( format!("cmap style requires 8 or 16 bit integer data in layer {}", config.name)))?;
                    let read_cmap = read_cmap.ok_or( misc_error( format!("no color map reader for layer {}", config.name)))?;
                    let entries = read_cmap( path)?;
                    for e in entries.iter().filter( |e| e.is_nodata) { add_unique( &mut nodata, e.value as f64); }
                    WmsStyle { name: "cmap", title: "color map", colorizer: Colorizer::Lut { lut: ValueLut::new( &entries, base), entries } }
                }
                WmsStyleConfig::Alpha{min,max} => {
                    WmsStyle { name: "alpha", title: "value mapped to alpha channel of COLOR", colorizer: Colorizer::Alpha{ min: *min, max: *max } }
                }
            };
            if styles.iter().any( |s| s.name == style.name) {
                return Err( misc_error( format!("duplicated style {} in layer {}", style.name, config.name)))
            }
            styles.push( style);
        }
        if styles.is_empty() { return Err( misc_error( format!("no styles for layer {}", config.name))) }

        Ok( WmsLayer {
            name: config.name.clone(),
            title: config.title.clone(),
            descr: config.descr.clone(),
            path: config.path.0.clone(),
            nodata,
            lut_base,
            geo_bounds,
            res_m,
            styles
        })
    }

    /// the style for the STYLES parameter of a GetMap request (empty or "default" means first configured style)
    pub fn style_for (&self, req: &WmsGetMap)->Option<&WmsStyle> {
        match req.styles.first().map( |s| s.as_str()) {
            None | Some("") | Some("default") => self.styles.first(),
            Some(name) => self.styles.iter().find( |s| s.name == name)
        }
    }

    /// JSON color map legend for layers that have a cmap style
    pub fn json_legend (&self)->Option<String> {
        self.styles.iter().find_map( |s| match &s.colorizer {
            Colorizer::Lut{entries,..} => Some( json_legend( entries)),
            _ => None
        })
    }

    /// a key that identifies the request specific style of the image (used for caching)
    pub fn style_key (&self, style: &WmsStyle, req: &WmsGetMap)->String {
        let nd = nodata_rgba( req);
        let nd = format!("{:02x}{:02x}{:02x}{:02x}", nd[0], nd[1], nd[2], nd[3]);

        match &style.colorizer {
            Colorizer::Lut{..} => format!("{}_{}", style.name, nd),
            Colorizer::Alpha{min,max} => {
                let (rgb, min, max) = alpha_params( *min, *max, req);
                format!("{}_{}_{:02x}{:02x}{:02x}_{}_{}", style.name, nd, rgb[0], rgb[1], rgb[2], min, max)
            }
        }
    }

    /// get the data value at the given position (in x,y order of srs) and the name of its color map entry (if any).
    /// Positions outside of the dataset return a nodata value. Note this is blocking (GDAL file access)
    pub fn feature_value (&self, srs: GeoDsSrs, x: f64, y: f64)->Result<FeatureValue> {
        let ds = Dataset::open( &self.path)?;
        let band = ds.rasterband(1)?;

        let mut src_srs = srs.to_spatial_ref()?;
        src_srs.set_axis_mapping_strategy( AxisMappingStrategy::TraditionalGisOrder);
        let mut ds_srs = ds.spatial_ref()?;
        ds_srs.set_axis_mapping_strategy( AxisMappingStrategy::TraditionalGisOrder);

        let (mut xs, mut ys) = ([x], [y]);
        CoordTransform::new( &src_srs, &ds_srs)?.transform_coords( &mut xs, &mut ys, &mut [])?;

        let gt = ds.geo_transform()?; // we assume north-up datasets (no rotation)
        let col = ((xs[0] - gt[0]) / gt[1]).floor();
        let row = ((ys[0] - gt[3]) / gt[5]).floor();
        let (n_cols, n_rows) = ds.raster_size();

        let value = if col >= 0.0 && row >= 0.0 && (col as usize) < n_cols && (row as usize) < n_rows {
            let buf = band.read_as::<f64>( (col as isize, row as isize), (1,1), (1,1), None)?;
            buf.data()[0]
        } else {
            band.no_data_value().unwrap_or( f64::NAN)
        };

        let is_nodata = crate::cmap::is_nodata( value, &self.nodata);
        let name = if is_nodata { None } else {
            self.styles.iter().find_map( |s| match &s.colorizer {
                Colorizer::Lut{entries,..} => entries.iter().find( |e| e.value as f64 == value).map( |e| e.name.clone()),
                _ => None
            })
        };

        Ok( FeatureValue { value, is_nodata, name } )
    }

    /// is the requested pixel size more than max_downsample times our data cell size
    pub fn is_too_coarse (&self, req: &WmsGetMap, max_downsample: f64)->bool {
        request_res_m( req) > self.res_m * max_downsample
    }

    /// compute a PNG image for the given GetMap request. Note this is blocking and should be called from a blocking context
    pub fn render_png (&self, style: &WmsStyle, req: &WmsGetMap)->Result<Vec<u8>> {
        let vs = self.read_values( req)?;
        let nd_rgba = nodata_rgba( req);

        let rgba = match &style.colorizer {
            Colorizer::Lut{lut,..} => lut.colorize( &vs, &self.nodata, nd_rgba),
            Colorizer::Alpha{min,max} => {
                let (rgb, min, max) = alpha_params( *min, *max, req);
                colorize_alpha( &vs, &self.nodata, nd_rgba, rgb, min, max)
            }
        };

        encode_png( &rgba, req.width, req.height)
    }

    /// warp the requested area into a MEM dataset of the requested size and return its values
    fn read_values (&self, req: &WmsGetMap)->Result<Vec<f64>> {
        let src_ds = Dataset::open( &self.path)?;
        let tgt_srs = SpatialRef::from_epsg( req.srs.epsg())?;

        let mut warp = SimpleWarpBuilder::new( &src_ds, "")?;
        warp.set_tgt_srs( &tgt_srs);
        warp.set_tgt_extent_from_bbox( &req.bbox);
        warp.set_tgt_size( req.width as i32, req.height as i32);
        warp.set_resample_alg( ResampleAlg::NearestNeighbour); // categorical data can't be interpolated, and we want to preserve data cells
        warp.set_tgt_format( "MEM")?;

        let tgt_ds = warp.exec()?;
        Ok( get_rasterband_as_vec::<f64>( &tgt_ds, 1)? )
    }
}

/// a fully transparent PNG image of the given size
pub fn transparent_png (width: u32, height: u32)->Result<Vec<u8>> {
    encode_png( &vec![0u8; (width * height * 4) as usize], width, height)
}

const M_PER_DEG_LAT: f64 = 110_574.0;
const M_PER_DEG_LON_EQUATOR: f64 = 111_320.0;

/// approximate max pixel size of a GetMap request in meters
fn request_res_m (req: &WmsGetMap)->f64 {
    let b = &req.bbox;
    let dx = (b.east - b.west) / req.width as f64;
    let dy = (b.north - b.south) / req.height as f64;

    match req.srs {
        GeoDsSrs::WGS84 | GeoDsSrs::CRS84 => { // bbox is already normalized to lon,lat
            let lat = ((b.north + b.south) / 2.0).to_radians();
            (dx * M_PER_DEG_LON_EQUATOR * lat.cos()).max( dy * M_PER_DEG_LAT)
        }
        GeoDsSrs::UTM{..} => dx.max(dy)
    }
}

/// approximate data cell size of a dataset in meters
fn get_res_m (ds: &Dataset, geo_bounds: &[f64;4])->Result<f64> {
    let gt = ds.geo_transform()?;
    let res = gt[1].abs().max( gt[5].abs());

    if ds.spatial_ref()?.is_geographic() {
        let lat = ((geo_bounds[1] + geo_bounds[3]) / 2.0).to_radians();
        Ok( res * M_PER_DEG_LON_EQUATOR * lat.cos())
    } else {
        Ok( res * ds.spatial_ref()?.linear_units()) // linear_units is meters per unit
    }
}

fn add_unique (vs: &mut Vec<f64>, v: f64) {
    if !vs.contains( &v) { vs.push(v) }
}

/// the smallest value of 8 and 16 bit integer data types, which can be colorized with value lookup tables
fn get_lut_base (data_type: GdalDataType)->Option<i32> {
    match data_type {
        GdalDataType::UInt8 | GdalDataType::UInt16 => Some(0),
        GdalDataType::Int8 => Some( i8::MIN as i32),
        GdalDataType::Int16 => Some( i16::MIN as i32),
        _ => None
    }
}

/// color for nodata pixels from the vendor specific NDCOLOR=RRGGBBAA parameter (default is transparent)
fn nodata_rgba (req: &WmsGetMap)->[u8;4] {
    req.params.get("ndcolor").and_then( |s| parse_rgba(s)).unwrap_or( [0,0,0,0])
}

/// color and value range for alpha styles from the vendor specific COLOR=RRGGBB and VRANGE=min,max parameters
fn alpha_params (mut min: f64, mut max: f64, req: &WmsGetMap)->([u8;3],f64,f64) {
    let rgb = req.params.get("color").and_then( |s| parse_rgb(s)).unwrap_or( DEFAULT_ALPHA_RGB);
    if let Some(vrange) = req.params.get("vrange") {
        let vs: Vec<f64> = vrange.split(',').filter_map( |s| s.trim().parse::<f64>().ok()).collect();
        if vs.len() == 2 && vs[0] < vs[1] { min = vs[0]; max = vs[1]; }
    }
    (rgb, min, max)
}

fn encode_png (rgba: &[u8], width: u32, height: u32)->Result<Vec<u8>> {
    let mut buf: Vec<u8> = Vec::with_capacity( rgba.len() / 4);
    let encoder = PngEncoder::new_with_quality( Cursor::new( &mut buf), CompressionType::Fast, FilterType::Adaptive);
    encoder.write_image( rgba, width, height, ExtendedColorType::Rgba8).map_err( |e| misc_error( format!("PNG encoding failed: {}", e)))?;
    Ok(buf)
}

/// geographic (EPSG:4326) bounds of a (projected) dataset
fn get_geo_bounds (ds: &Dataset)->Result<[f64;4]> {
    let (cols, rows) = ds.raster_size();
    let gt = ds.geo_transform()?;
    let x0 = gt[0];
    let y0 = gt[3];
    let x1 = x0 + gt[1] * cols as f64;
    let y1 = y0 + gt[5] * rows as f64;
    let bounds = [ x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1) ];

    let mut src_srs = ds.spatial_ref()?;
    src_srs.set_axis_mapping_strategy( AxisMappingStrategy::TraditionalGisOrder);
    let mut tgt_srs = SpatialRef::from_epsg( 4326)?;
    tgt_srs.set_axis_mapping_strategy( AxisMappingStrategy::TraditionalGisOrder);

    let ct = CoordTransform::new( &src_srs, &tgt_srs)?;
    Ok( ct.transform_bounds( &bounds, 21)? )
}

/// load all configured layers. Layers that can't be loaded are reported and skipped
pub fn load_layers (config: &WmsConfig, read_cmap: Option<&CmapReader>)->HashMap<String,WmsLayer> {
    let mut layers = HashMap::new();
    for lc in &config.layers {
        match WmsLayer::new( lc, read_cmap) {
            Ok(layer) => { layers.insert( lc.name.clone(), layer); }
            Err(e) => eprintln!("failed to load WMS layer {}: {}", lc.name, e)
        }
    }
    layers
}

/// cache path for a GetMap request
pub fn get_cache_path (cache_dir: &Path, layer: &WmsLayer, style: &WmsStyle, req: &WmsGetMap)->PathBuf {
    let b = &req.bbox;
    let fname = format!("{}_{:.7},{:.7},{:.7},{:.7}_{}x{}.png", req.srs.epsg(), b.west, b.south, b.east, b.north, req.width, req.height);
    cache_dir.join( &layer.name).join( layer.style_key( style, req)).join( fname)
}

/// generate WMS 1.3.0 capabilities for the configured layers (in config order). The online_resource is the absolute URL
/// of our WMS endpoint
pub fn capabilities_xml (config: &WmsConfig, layers: &HashMap<String,WmsLayer>, online_resource: &str)->String {
    let url = xml_escape( online_resource);
    let mut s = String::with_capacity( 4096);

    s.push_str( &format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<WMS_Capabilities version="1.3.0" xmlns="http://www.opengis.net/wms" xmlns:xlink="http://www.w3.org/1999/xlink">
  <Service>
    <Name>WMS</Name>
    <Title>{title}</Title>
    <OnlineResource xlink:type="simple" xlink:href="{url}"/>
  </Service>
  <Capability>
    <Request>
      <GetCapabilities>
        <Format>text/xml</Format>
        <DCPType><HTTP><Get><OnlineResource xlink:type="simple" xlink:href="{url}?"/></Get></HTTP></DCPType>
      </GetCapabilities>
      <GetMap>
        <Format>image/png</Format>
        <DCPType><HTTP><Get><OnlineResource xlink:type="simple" xlink:href="{url}?"/></Get></HTTP></DCPType>
      </GetMap>
      <GetFeatureInfo>
        <Format>application/json</Format>
        <Format>application/geo+json</Format>
        <DCPType><HTTP><Get><OnlineResource xlink:type="simple" xlink:href="{url}?"/></Get></HTTP></DCPType>
      </GetFeatureInfo>
    </Request>
    <Exception>
      <Format>XML</Format>
    </Exception>
    <Layer>
      <Title>{title}</Title>
      <CRS>EPSG:4326</CRS>
      <CRS>CRS:84</CRS>
"#, title = xml_escape( &config.title)));

    let lcs: Vec<&WmsLayer> = config.layers.iter().filter_map( |lc| layers.get( &lc.name)).collect();
    if let Some(b) = union_bounds( &lcs) {
        s.push_str( &geo_bbox_xml( &b, "      "));
    }

    for layer in lcs {
        s.push_str( &format!("      <Layer queryable=\"1\" opaque=\"0\">\n        <Name>{}</Name>\n        <Title>{}</Title>\n",
                             xml_escape( &layer.name), xml_escape( &layer.title)));
        if let Some(descr) = &layer.descr {
            s.push_str( &format!("        <Abstract>{}</Abstract>\n", xml_escape( descr)));
        }
        s.push_str( &geo_bbox_xml( &layer.geo_bounds, "        "));
        for style in &layer.styles {
            s.push_str( &format!("        <Style>\n          <Name>{}</Name>\n          <Title>{}</Title>\n        </Style>\n", style.name, xml_escape( style.title)));
        }
        s.push_str( "      </Layer>\n");
    }

    s.push_str( "    </Layer>\n  </Capability>\n</WMS_Capabilities>\n");
    s
}

fn union_bounds (layers: &[&WmsLayer])->Option<[f64;4]> {
    layers.iter().map( |l| l.geo_bounds).reduce( |a,b| [ a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3]) ])
}

fn geo_bbox_xml (b: &[f64;4], indent: &str)->String {
    let (w,s,e,n) = (b[0], b[1], b[2], b[3]);
    format!(r#"{indent}<EX_GeographicBoundingBox>
{indent}  <westBoundLongitude>{w:.6}</westBoundLongitude>
{indent}  <eastBoundLongitude>{e:.6}</eastBoundLongitude>
{indent}  <southBoundLatitude>{s:.6}</southBoundLatitude>
{indent}  <northBoundLatitude>{n:.6}</northBoundLatitude>
{indent}</EX_GeographicBoundingBox>
{indent}<BoundingBox CRS="CRS:84" minx="{w:.6}" miny="{s:.6}" maxx="{e:.6}" maxy="{n:.6}"/>
{indent}<BoundingBox CRS="EPSG:4326" minx="{s:.6}" miny="{w:.6}" maxx="{n:.6}" maxy="{e:.6}"/>
"#)
}

/* #endregion WMS layers */

/* #region WmsService ****************************************************************************************/

/// the state and request handlers of a WMS endpoint. This is supposed to be shared (Arc) between the route handlers
/// of the SpaService that uses it
pub struct WmsService {
    pub config: WmsConfig,
    pub layers: HashMap<String,WmsLayer>,
    cache_dir: PathBuf,
    n_cached: AtomicUsize,

    // limits the number of concurrent GetMap computations. Requests waiting for a permit are just pending futures, i.e.
    // they are dropped without any work if the client cancels the request (which Cesium does a lot while the camera moves).
    // Tasks spawned with spawn_blocking() cannot be cancelled, i.e. without this limit cancelled requests pile up and
    // run out of file handles
    render_permits: Semaphore,
}

impl WmsService {
    /// create a new service, loading all configured layers. Layers that can't be loaded are reported and skipped.
    /// The read_cmap function is required if layers have cmap styles
    pub fn new (config: WmsConfig, cache_dir: PathBuf, read_cmap: Option<&CmapReader>)->Self {
        let layers = load_layers( &config, read_cmap);
        let render_permits = Semaphore::new( config.max_renders.max(1));
        WmsService { config, layers, cache_dir, n_cached: AtomicUsize::new(0), render_permits }
    }

    /// handler for WMS requests (GetCapabilities, GetMap)
    pub async fn handle_wms (self: &Arc<Self>, query: &HashMap<String,String>, headers: &HeaderMap, uri: &Uri)->Response {
        match parse_wms_request( query) {
            Ok(WmsRequest::GetMap(gm)) => self.get_map( gm).await,
            Ok(WmsRequest::GetFeatureInfo(gfi)) => self.get_feature_info( gfi).await,
            Ok(WmsRequest::GetCapabilities) => {
                let xml = capabilities_xml( &self.config, &self.layers, &online_resource( headers, uri));
                (StatusCode::OK, [(header::CONTENT_TYPE, "text/xml")], xml).into_response()
            }
            Err(e) => e.into_response()
        }
    }

    /// handler for JSON color map legend requests of layers with cmap styles ("<layer>" or "<layer>.json")
    pub fn handle_legend (&self, layer_name: &str)->Response {
        let layer_name = layer_name.strip_suffix(".json").unwrap_or( layer_name);
        if let Some(layer) = self.layers.get( layer_name) && let Some(json) = layer.json_legend() {
            (StatusCode::OK, [(header::CONTENT_TYPE, "application/json")], json).into_response()
        } else {
            (StatusCode::NOT_FOUND, "unknown color map").into_response()
        }
    }

    async fn get_map (self: &Arc<Self>, gm: WmsGetMap)->Response {
        if gm.layers.len() != 1 {
            return WmsException::new( Some("LayerNotDefined"), "only single layer requests are supported").into_response()
        }
        let Some(layer) = self.layers.get( &gm.layers[0]) else {
            return WmsException::new( Some("LayerNotDefined"), format!("unknown layer {}", gm.layers[0])).into_response()
        };
        if !matches!( gm.img_type, GeoDsImgType::PNG) {
            return WmsException::new( Some("InvalidFormat"), "only image/png is supported").into_response()
        }
        if gm.width > MAX_IMG_SIZE || gm.height > MAX_IMG_SIZE {
            return WmsException::new( Some("InvalidParameterValue"), format!("max image size is {MAX_IMG_SIZE}")).into_response()
        }
        if layer.style_for( &gm).is_none() {
            return WmsException::new( Some("StyleNotDefined"), format!("unknown style for layer {}", layer.name)).into_response()
        }

        // don't compute images that are much coarser than the data resolution. Computation time is proportional to the
        // source area, i.e. a few of those would block all render slots for a long time (and with it all the requests
        // waiting for them, including other browser requests to this server since browsers limit the number of connections)
        if layer.is_too_coarse( &gm, self.config.max_downsample) {
            return match transparent_png( gm.width, gm.height) {
                Ok(png) => png_response( png),
                Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "failed to create image").into_response()
            }
        }

        // wait for a free render slot (queued in FIFO order). This is dropped if the client cancels the request
        let Ok(_permit) = self.render_permits.acquire().await else {
            return (StatusCode::SERVICE_UNAVAILABLE, "WMS not available").into_response()
        };

        // GDAL calls and file IO are blocking, i.e. we must not execute them on async runtime worker threads
        let res = tokio::task::spawn_blocking({
            let svc = self.clone();
            move || svc.get_png( &gm)
        }).await;

        match res {
            Ok(Ok(png)) => png_response( png),
            Ok(Err(e)) => {
                eprintln!("WMS GetMap failed: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, "failed to create image").into_response()
            }
            Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "image task failed").into_response()
        }
    }

    async fn get_feature_info (self: &Arc<Self>, gfi: WmsGetFeatureInfo)->Response {
        if let Some(name) = gfi.query_layers.iter().find( |l| !self.layers.contains_key( *l)) {
            return WmsException::new( Some("LayerNotDefined"), format!("unknown layer {name}")).into_response()
        }

        // single pixel reads are cheap, i.e. we don't need render permits but GDAL calls are still blocking
        let res = tokio::task::spawn_blocking({
            let svc = self.clone();
            move || svc.feature_info_json( &gfi)
        }).await;

        match res {
            Ok(Ok(json)) => (StatusCode::OK, [(header::CONTENT_TYPE, "application/json")], json).into_response(),
            Ok(Err(e)) => {
                eprintln!("WMS GetFeatureInfo failed: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, "failed to get feature info").into_response()
            }
            Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "feature info task failed").into_response()
        }
    }

    /// GeoJSON FeatureCollection with one Point feature per query layer. Properties are "layer", "value" (null for
    /// nodata), "nodata" and "name" (color map entry name, the layer title for non-categorical data)
    fn feature_info_json (&self, gfi: &WmsGetFeatureInfo)->Result<String> {
        let (x, y) = gfi.position();
        let mut features: Vec<serde_json::Value> = Vec::with_capacity( gfi.query_layers.len());

        for layer_name in &gfi.query_layers {
            let layer = self.layers.get( layer_name).ok_or( misc_error( format!("unknown layer {layer_name}")))?; // already checked
            let fv = layer.feature_value( gfi.map.srs, x, y)?;

            let value = if fv.is_nodata { serde_json::Value::Null } else { serde_json::json!( fv.value) };
            let name = fv.name.unwrap_or_else( || if fv.is_nodata { "nodata".to_string() } else { layer.title.clone() });
            let geometry = if matches!( gfi.map.srs, GeoDsSrs::WGS84 | GeoDsSrs::CRS84) {
                serde_json::json!({ "type": "Point", "coordinates": [x, y] })
            } else {
                serde_json::Value::Null // GeoJSON requires geographic coordinates
            };

            features.push( serde_json::json!({
                "type": "Feature",
                "properties": { "layer": layer_name, "value": value, "nodata": fv.is_nodata, "name": name },
                "geometry": geometry
            }));
        }

        Ok( serde_json::json!({ "type": "FeatureCollection", "features": features }).to_string())
    }

    /// get PNG image for request, either from cache or by rendering it from the layer dataset (blocking)
    fn get_png (&self, gm: &WmsGetMap)->Result<Vec<u8>> {
        let layer = self.layers.get( &gm.layers[0]).ok_or( misc_error("unknown layer".into()))?; // already checked
        let style = layer.style_for( gm).ok_or( misc_error("unknown style".into()))?; // already checked
        let path = get_cache_path( &self.cache_dir, layer, style, gm);

        if path.is_file() {
            fs::set_accessed( &path); // for LRU purging
            Ok( std::fs::read( &path)? )

        } else {
            let png = layer.render_png( style, gm)?;

            if let Some(dir) = path.parent() { std::fs::create_dir_all( dir)?; }
            std::fs::write( &path, &png)?;

            if self.n_cached.fetch_add( 1, Ordering::Relaxed) % PURGE_INTERVAL == PURGE_INTERVAL-1 {
                if let Err(e) = fs::purge_lru_files_above_limit( &self.cache_dir, true, self.config.max_cache) {
                    eprintln!("failed to purge WMS cache: {e}");
                }
            }
            Ok(png)
        }
    }
}

fn png_response (png: Vec<u8>)->Response {
    (StatusCode::OK, [(header::CONTENT_TYPE, "image/png")], png).into_response()
}

/// the absolute URL of our WMS endpoint as seen by the client (for capabilities)
fn online_resource (headers: &HeaderMap, uri: &Uri)->String {
    let scheme = headers.get("x-forwarded-proto").and_then( |v| v.to_str().ok()).unwrap_or("http");
    let host = headers.get( header::HOST).and_then( |v| v.to_str().ok()).unwrap_or("localhost");
    format!("{}://{}{}", scheme, host, uri.path())
}

/* #endregion WmsService */
