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

//! WMS support for Landfire data sets. GetMap images are computed directly from the original (tiled) landfire.gov
//! GeoTIFFs, i.e. we warp the requested area into a MEM dataset (nearest neighbour, preserving the data resolution)
//! and colorize it with a value lookup table:
//!  - categorical data sets (EVT, FBFM40, CC, ..) use landfire.gov CSV color maps
//!  - continuous data sets (Asp, SlpD, Elev) use a single (requested) color with the value mapped to the alpha channel
//! This does not require any derived data products, which simplifies updates of the (quarterly released) data sets

use std::{collections::HashMap, io::Cursor, path::{Path,PathBuf}};
use gdal::{Dataset, spatial_ref::{AxisMappingStrategy, CoordTransform, SpatialRef}};
use image::{ImageEncoder, codecs::png::{CompressionType, FilterType, PngEncoder}, ExtendedColorType};
use serde::Deserialize;

use odin_common::{BoundingBox, fs::EnvPathBuf};
use odin_gdal::{get_rasterband_as_vec, geods::GeoDsSrs, geods_srv::{WmsGetMap, xml_escape}, warp::{ResampleAlg, SimpleWarpBuilder}};

use crate::{cmap::{CsvColorEntry, ValueLut, colorize_alpha, odin_json_color_map, parse_rgb, parse_rgba, read_csv_color_map}, errors::{OdinLandfireError, Result, op_failed}};

/// default color for alpha encoded layers if the request does not specify one (vendor specific COLOR parameter)
const DEFAULT_ALPHA_RGB: [u8;3] = [255,255,255];

/// the static WMS configuration (part of LandfireConfig)
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
    pub nodata: Vec<i16>, // additional nodata values (the declared band nodata value is always included)
    pub styles: Vec<WmsStyleConfig>, // the first one is the default style
}

/// the supported WMS styles (colorizations). Each one can only be used once per layer since the style name is
/// derived from the type ("csv" or "alpha")
#[derive(Deserialize,Debug)]
pub enum WmsStyleConfig {
    /// categorical data with landfire.gov CSV color map
    Csv(EnvPathBuf),
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
    pub nodata: Vec<i16>, // declared nodata, configured nodata values and "Fill-NoData" values of CSV color maps
    pub geo_bounds: [f64;4], // west,south,east,north in degrees
    pub res_m: f64, // approximate data cell size in meters
    pub styles: Vec<WmsStyle>,
}

pub struct WmsStyle {
    pub name: &'static str,
    pub title: &'static str,
    pub colorizer: Colorizer,
}

pub enum Colorizer {
    Lut { lut: ValueLut, entries: Vec<CsvColorEntry> },
    Alpha { min: f64, max: f64 },
}

impl WmsLayer {
    pub fn new (config: &WmsLayerConfig)->Result<Self> {
        let ds = Dataset::open( &config.path)?;
        let geo_bounds = get_geo_bounds( &ds)?;
        let res_m = get_res_m( &ds, &geo_bounds)?;

        // note that nodata pixels outside of the dataset extent are set to the declared nodata value by the warp
        let mut nodata: Vec<i16> = config.nodata.clone();
        if let Some(v) = ds.rasterband(1)?.no_data_value() { add_unique( &mut nodata, v as i16); }

        let mut styles: Vec<WmsStyle> = Vec::with_capacity( config.styles.len());
        for sc in &config.styles {
            let style = match sc {
                WmsStyleConfig::Csv(path) => {
                    let entries = read_csv_color_map( path)?;
                    for e in entries.iter().filter( |e| e.is_nodata()) { add_unique( &mut nodata, e.value as i16); }
                    WmsStyle { name: "csv", title: "landfire.gov color map", colorizer: Colorizer::Lut { lut: ValueLut::from_entries( &entries), entries } }
                }
                WmsStyleConfig::Alpha{min,max} => {
                    WmsStyle { name: "alpha", title: "value mapped to alpha channel of COLOR", colorizer: Colorizer::Alpha{ min: *min, max: *max } }
                }
            };
            if styles.iter().any( |s| s.name == style.name) {
                return Err( op_failed!("duplicated style {} in layer {}", style.name, config.name))
            }
            styles.push( style);
        }
        if styles.is_empty() { return Err( op_failed!("no styles for layer {}", config.name)) }

        Ok( WmsLayer {
            name: config.name.clone(),
            title: config.title.clone(),
            descr: config.descr.clone(),
            path: config.path.0.clone(),
            nodata,
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

    /// ODIN JSON color map (legend) for layers that have a CSV style
    pub fn json_color_map (&self)->Option<String> {
        self.styles.iter().find_map( |s| match &s.colorizer {
            Colorizer::Lut{entries,..} => Some( odin_json_color_map( entries)),
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
    fn read_values (&self, req: &WmsGetMap)->Result<Vec<i16>> {
        let src_ds = Dataset::open( &self.path)?;
        let tgt_srs = SpatialRef::from_epsg( req.srs.epsg())?;

        let mut warp = SimpleWarpBuilder::new( &src_ds, "")?;
        warp.set_tgt_srs( &tgt_srs);
        warp.set_tgt_extent_from_bbox( &req.bbox);
        warp.set_tgt_size( req.width as i32, req.height as i32);
        warp.set_resample_alg( ResampleAlg::NearestNeighbour); // categorical data - we can't interpolate
        warp.set_tgt_format( "MEM")?;

        let tgt_ds = warp.exec()?;
        Ok( get_rasterband_as_vec::<i16>( &tgt_ds, 1)? )
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

fn add_unique (vs: &mut Vec<i16>, v: i16) {
    if !vs.contains( &v) { vs.push(v) }
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
    encoder.write_image( rgba, width, height, ExtendedColorType::Rgba8).map_err( |e| op_failed!("PNG encoding failed: {}", e))?;
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
pub fn load_layers (config: &WmsConfig)->HashMap<String,WmsLayer> {
    let mut layers = HashMap::new();
    for lc in &config.layers {
        match WmsLayer::new( lc) {
            Ok(layer) => { layers.insert( lc.name.clone(), layer); }
            Err(e) => eprintln!("failed to load landfire WMS layer {}: {}", lc.name, e)
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
        s.push_str( &format!("      <Layer queryable=\"0\" opaque=\"0\">\n        <Name>{}</Name>\n        <Title>{}</Title>\n",
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
