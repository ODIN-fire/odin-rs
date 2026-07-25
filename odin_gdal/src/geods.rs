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

use std::{path::{Path,PathBuf},fs::File, collections::HashMap};
use odin_server::errors::op_failed;
use serde::{Deserialize, Serialize};
use reqwest::Client;
use axum::{http::{header::CONTENT_TYPE, HeaderMap, HeaderValue}, response::IntoResponse, body::Body};
use tokio;
use tokio_util;
use gdal;

use odin_build;
use odin_common::{fs::{self,EnvPathBuf}, net::{download_url, mime_type_for_extension, NO_HEADERS}, BoundingBox};
use crate::{
    CACHE_DIR, load_config, get_values_for_ds_positions, get_driver_name_for_extension, csl_string_list, create_wh_image_from_ds,
    Dataset, SpatialRef, CslStringList,
    warp::{SimpleWarpBuilder},
    errors::{Result,OdinGdalError,invalid_filename,op_failed}
};

/// this is the config struct the GeoDs filenames refer to
/// it abstracts over local filesystem or external server access. The former is significantly faster
/// but the latter might be required in case the underlying data is too large for user servers (as in
/// the case of large DEMs that can easily end up with tens of gigabytes disk space).
/// Clients should not have to be aware of the actual implementation.
///
#[derive(Deserialize,Debug)]
pub enum GeoDs {
    File { layers: HashMap<String,EnvPathBuf> },
    Server { url: String, app_key: Option<String> }
}

impl GeoDs {

    /// get values for a list of positions
    pub async fn get_values (&self, layer: &str, no_data: Option<f64>, srs: &GeoDsSrs, locations: &mut [(f64,f64)]) -> Result<Vec<f64>> {
        match self {
            GeoDs::Server{url,app_key} => {
                let ak = app_key.as_ref().map(|k| k.as_str());
                request_values( url.as_str(), ak, layer, no_data, srs, locations).await
            }
            GeoDs::File{layers} => {
                if let Some(ds_file) = layers.get(layer) {
                    read_values( ds_file, no_data, srs, locations)
                } else { Err( op_failed!("unknown layer {}", layer) ) }
            }
        }
    }

    // get a file containing the whole raster for a given bounding box and target resolution in a given format and stored in a given path
    pub async fn get_res_file<P> (&self, layer: &str, bbox: &BoundingBox<f64>, srs: GeoDsSrs, res_x: f64, res_y: f64, no_data: Option<f64>, img_type: GeoDsImgType, file_path: P) -> Result<()>
        where P: AsRef<Path>
    {
        match self {
            GeoDs::Server{url,app_key} => {
                let ak = app_key.as_ref().map(|k| k.as_str());
                request_res_file(url, ak, layer, bbox, srs, res_x, res_y, no_data, img_type, file_path).await
            }
            GeoDs::File{layers} => {
                if let Some(ds_file) = layers.get(layer) {
                    read_res_file( bbox, srs, res_x, res_y, no_data, img_type, ds_file, file_path)
                } else { Err( op_failed!("unknown layer {}", layer) ) }
            }
        }
    }

    // get a file containing the whole raster for a given bounding box and target resolution in a given format and stored in a given path
    pub async fn get_wh_file<P> (&self, layer: &str, bbox: &BoundingBox<f64>, srs: GeoDsSrs, w: u32, h: u32, no_data: Option<f64>, img_type: GeoDsImgType, file_path: P) -> Result<()>
        where P: AsRef<Path>
    {
        match self {
            GeoDs::Server{url,app_key} => {
                let ak = app_key.as_ref().map(|k| k.as_str());
                request_wh_file(url, ak, layer, bbox, srs, w, h, no_data, img_type, file_path).await
            }
            GeoDs::File{layers} => {
                if let Some(ds_file) = layers.get(layer) {
                    read_wh_file( bbox, srs, w, h, no_data, img_type, ds_file, file_path)
                } else { Err( op_failed!("unknown layer {}", layer) ) }
            }
        }
    }

    //.. possibly more to follow
}

async fn request_values (base_url: &str, app_key: Option<&str>, layer: &str, no_data: Option<f64>, srs: &GeoDsSrs, pts: &[(f64,f64)]) -> Result<Vec<f64>> {
    let url = match no_data {
        Some(no_data_value) => format!("{base_url}/GetValues?epsg={}&nodata={}&layer={}", srs.epsg(), no_data_value,layer),
        None => format!("{base_url}/GetValues?epsg={}&layer={}", srs.epsg(), layer)
    };

    let client = Client::new();
    let values: Vec<f64> = odin_common::net::post_json_query( &client, &url, pts).await?;

    Ok(values)
}

pub fn read_values<P> (ds_file: P, no_data: Option<f64>, srs: &GeoDsSrs, pts: &mut [(f64,f64)]) -> Result<Vec<f64>>
    where P: AsRef<Path>
{
    let ds_path = ds_file.as_ref();
    ds_path.try_exists()?;
    let sr = srs.to_spatial_ref()?;

    Ok( get_values_for_ds_positions( ds_path, 1, no_data, &sr, pts)? )
}

/// for a given bounding box 'bbox' look for a matching file in 'cache_dir'.
/// If none found yet create a file with the given 'img_type' from the virtual GDAL tileset specified by 'ds_file'
/// compute width and height from given res_x and res_y resolutions
/// note that `bbox` has to be in `srs` units (degree for GEO, meters for UTM)
pub fn read_res_file<P,Q> (
    bbox: &BoundingBox<f64>,
    srs: GeoDsSrs,
    res_x: f64, res_y: f64,
    nodata: Option<f64>,
    img_type: GeoDsImgType,
    ds_file: P,
    res_file: Q
) -> Result<()>
    where P: AsRef<Path>, Q: AsRef<Path>
{
    let ds_path = ds_file.as_ref();
    ds_path.try_exists()?;

    if !res_file.as_ref().is_file() {
        let create_opts = img_type.gdal_create_options();
        let img_extension = img_type.file_extension();
        let driver_name = get_driver_name_for_extension( img_extension).ok_or(OdinGdalError::MiscError(format!("unsupported image type {}", img_extension)))?;
        let src_ds = Dataset::open(&ds_path)?;
        let tgt_srs = SpatialRef::from_epsg( srs.epsg())?;

        let mut warp = SimpleWarpBuilder::new( &src_ds, res_file)?;

        warp.set_tgt_srs( &tgt_srs);
        warp.set_tgt_extent_from_bbox( bbox);
        warp.set_tgt_resolution( res_x, res_y);
        warp.set_tgt_format( driver_name)?;

        // NOTE - we do need explicit control over nodata (e.g. for DEM cases that would get thrown off by -9999)
        // make sure we use a small but non-zero value for nodata since we can encounter both (0 for coastal sea and nodata for high seas)
        // note that the nodata value might get used subsequently in SRS conversion / cropping
        if let Some(nodata) = nodata {
            warp.set_tgt_nodatas(vec![nodata]);
        }

        if let Some(opts) = &create_opts { warp.set_create_options( opts); }
        warp.exec()?;

    } else {
        fs::set_accessed( &res_file)?; // update atime so that we could use it for LRU cache bounds
    }

    Ok(())
}

/// send request to a GeoDs server
async fn request_res_file <P> (
    base_url: &str, app_key: Option<&str>,
    layer: &str,
    bbox: &BoundingBox<f64>,
    srs: GeoDsSrs,
    res_x: f64, res_y: f64,
    nodata: Option<f64>,
    img_type: GeoDsImgType,
    file_path: P
) -> Result<()>
    where P: AsRef<Path>
{
    if !file_path.as_ref().is_file() {
        let url = match nodata {
            Some(nodata) => {
                format!("{}/GetResGrid?crs=EPSG:{}&bbox={:.5},{:.5},{:.5},{:.5}&res_x={}&res_y={}&format=image/tif&layer={}&nodata={}",
                        base_url, srs.epsg(), bbox.west, bbox.south, bbox.east, bbox.north, res_x, res_y, layer, nodata)
            }
            None => {
                format!("{}/GetResGrid?crs=EPSG:{}&bbox={:.5},{:.5},{:.5},{:.5}&res_x={}&res_y={}&format=image/tif&layer={}",
                        base_url, srs.epsg(), bbox.west, bbox.south, bbox.east, bbox.north, res_x, res_y, layer)
            }
        };


        let client = reqwest::Client::new();
        download_url( &client, &url, NO_HEADERS, file_path).await?;
        // we could check here if the length is > 0

    } else {
        fs::set_accessed(&file_path)?; // update atime so that we could use it for LRU cache bounds
    }

    Ok(())
}

/// for a given bounding box 'bbox' look for a matching file in 'cache_dir'.
/// If none found yet create a file with the given 'img_type' from the virtual GDAL tileset specified by 'ds_file'
/// compute res_x and res_y resolutions from given width and height
/// note that `bbox` has to be in `srs` units (degree for GEO, meters for UTM)
pub fn read_wh_file<P,Q> (
    bbox: &BoundingBox<f64>,
    srs: GeoDsSrs,
    width: u32, height: u32,
    nodata: Option<f64>,
    img_type: GeoDsImgType,
    ds_file: P,
    res_file: Q
) -> Result<()>
    where P: AsRef<Path>, Q: AsRef<Path>
{
    let ds_path = ds_file.as_ref();
    ds_path.try_exists()?; // check if the input ds file exists

    if let Some(dir) = res_file.as_ref().parent() {
        if !dir.is_dir() {
            std::fs::create_dir_all(dir)?;
        }
    }

    if !res_file.as_ref().is_file() {
        let create_opts = img_type.gdal_create_options();
        create_wh_image_from_ds( bbox, srs.epsg(), width, height, img_type.file_extension(), &create_opts, &ds_path, res_file)?;
    } else {
        fs::set_accessed(&res_file)?; // update atime so that we could use it for LRU cache bounds
    }

    Ok(())
}

/// send request to a GeoDs server
async fn request_wh_file <P> (
    base_url: &str, app_key: Option<&str>,
    layer: &str,
    bbox: &BoundingBox<f64>,
    srs: GeoDsSrs,
    w: u32, h: u32,
    nodata: Option<f64>,
    img_type: GeoDsImgType,
    file_path: P
) -> Result<()>
    where P: AsRef<Path>
{
    if !file_path.as_ref().is_file() {
        let url = match nodata {
            Some(nodata) => {
                format!("{}/GetWhGrid?crs=EPSG:{}&bbox={:.5},{:.5},{:.5},{:.5}&width={}&height={}&format=image/tif&layer={}&nodata={}",
                        base_url, srs.epsg(), bbox.west, bbox.south, bbox.east, bbox.north, w, h, layer, nodata)
            }
            None => {
                format!("{}/GetWhGrid?crs=EPSG:{}&bbox={:.5},{:.5},{:.5},{:.5}&width={}&height={}&format=image/tif&layer={}",
                        base_url, srs.epsg(), bbox.west, bbox.south, bbox.east, bbox.north, w, h, layer)
            }
        };

        let client = reqwest::Client::new();
        download_url( &client, &url, NO_HEADERS, file_path).await?;
        // we could check here if the length is > 0

    } else {
        fs::set_accessed(&file_path)?; // update atime so that we could use it for LRU cache bounds
    }

    Ok(())
}

#[derive(Debug,Clone,Copy)]
pub enum GeoDsImgType {
    PNG,
    TIF,
    JPEG,
}

impl GeoDsImgType {
    pub fn for_ext (file_ext: &str) -> Option<GeoDsImgType> {
        match file_ext {
            "tif" => Some(GeoDsImgType::TIF),
            "png" => Some(GeoDsImgType::PNG),
            "jpeg" => Some(GeoDsImgType::JPEG),
            _ => None
        }
    }

    pub fn for_mime_type (mime_type: &str) -> Option<GeoDsImgType> {
        match mime_type {
            "image/tif" => Some(GeoDsImgType::TIF),
            "image/png" => Some(GeoDsImgType::PNG),
            "image/jpeg" => Some(GeoDsImgType::JPEG),
            _ => None
        }
    }

    pub fn file_extension(&self) -> &'static str {
        match *self {
            GeoDsImgType::PNG => "png",
            GeoDsImgType::TIF => "tif",
            GeoDsImgType::JPEG => "jpg",
        }
    }

    // unfortunately we can't do this as a static ref since CslStringList has mutable fields
    pub fn gdal_create_options(&self) -> Option<CslStringList> {
        match *self {
            GeoDsImgType::PNG => None,
            GeoDsImgType::JPEG => None,
            GeoDsImgType::TIF => Some( csl_string_list!("COMPRESS=DEFLATE", "PREDICTOR=2").unwrap() ) // Ok to panic since this is static
        }
    }

    pub fn gdal_driver_name(&self) -> &'static str {
        get_driver_name_for_extension( self.file_extension()).expect("unsupported GDAL image type")
    }

    pub fn content_type(&self) -> &'static str {
        mime_type_for_extension( &self.file_extension()).expect("unknown mime type")
    }
}

/// the spatial reference systems geods can convert to
#[derive(Clone,Copy,Debug)]
pub enum GeoDsSrs {
    WGS84, // lat,lon
    CRS84, // lon,lat
    UTM { epsg: u32 },
}

impl GeoDsSrs {

    // NOTE - there is no specific EPSG for CRS84, it is the same as 4326 but with lon,lat order
    pub fn from_epsg (epsg: u32) -> Option<GeoDsSrs> {
        match epsg {
            4326 => Some(GeoDsSrs::WGS84),
            32601..32661 => Some(GeoDsSrs::UTM{epsg}),
            32701..32761 => Some(GeoDsSrs::UTM{epsg}),
            _ => None
        }
    }

    pub fn from_srs_spec (srs: &str) -> Option<GeoDsSrs> {
        if srs.starts_with("epsg:") || srs.starts_with("EPSG:") {
            if let Ok(epsg) = srs[5..].parse::<u32>() {
                return Self::from_epsg( epsg)
            }
        } else {
            if let Ok(epsg) = srs.parse::<u32>() {
                return Self::from_epsg( epsg)
            } else {
                if srs.eq_ignore_ascii_case("WGS84") {
                    return Some(GeoDsSrs::WGS84)
                }
                if srs.eq_ignore_ascii_case("CRS:84") {
                    return Some(GeoDsSrs::CRS84)
                }
            }

            // TODO - support more specs
        }

        None
    }

    pub fn epsg(&self) -> u32 {
        match *self {
            GeoDsSrs::WGS84 => 4326,
            GeoDsSrs::CRS84 => 4326, // NOTE - coordinate order has to be ensured by caller
            GeoDsSrs::UTM{epsg} => epsg,
        }
    }

    pub fn to_spatial_ref (&self)->Result<SpatialRef> {
        Ok( SpatialRef::from_epsg( self.epsg())? )
    }
}

#[derive(Debug,Serialize,Deserialize,Clone)]
pub struct Resolution {
    pub width: u64,
    pub height: u64
}

//--- server utilities

/// HTTP response creation
async fn create_response (file: File, img_type: GeoDsImgType) -> impl IntoResponse {
    let f = tokio::fs::File::from_std(file);
    let stream = tokio_util::io::ReaderStream::new(f);
    let body = Body::from_stream(stream);

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(img_type.content_type()));

    (headers,body)
}


// note that we do not need a semantic 'topic' here since we can cache based on the vrt_file name
pub fn get_local_res_file_path<P> ( vrt_file: &P, bbox: &BoundingBox<f64>, srs: GeoDsSrs, res_x: f64, res_y: f64,
                                    img_type: GeoDsImgType) -> Result<PathBuf>
    where P: AsRef<Path>
{
    let data_src = fs::basename( vrt_file).ok_or( invalid_filename( vrt_file))?;
    let fname = get_res_filename( srs.epsg(), bbox, res_x, res_y, img_type.file_extension());

    let mut fpath = CACHE_DIR.join(data_src);
    fpath.push(fname);
    Ok( fpath )
}

pub fn get_res_filename (epsg: u32, bbox: &BoundingBox<f64>, res_x: f64, res_y: f64, file_ext: &str) -> String {
    format!("{epsg}_{},{},{},{}_{res_x},{res_y}.{file_ext}", bbox.west, bbox.south, bbox.east, bbox.north)
}

pub fn get_local_wh_file_path<P> ( ds_file: &P, bbox: &BoundingBox<f64>, srs: GeoDsSrs, width: u32, height: u32,
                                   img_type: GeoDsImgType) -> Result<PathBuf>
    where P: AsRef<Path>
{
    let data_src = fs::basename( ds_file).ok_or(invalid_filename( format!("{:?}", ds_file.as_ref())))?;
    let fname = get_wh_filename( srs.epsg(), bbox, width, height, img_type.file_extension());

    let mut fpath = CACHE_DIR.join(data_src);
    fpath.push(fname);
    Ok( fpath )
}

pub fn get_local_wh_overlay_file_path<P> ( ds_files: &[&P], bbox: &BoundingBox<f64>, srs: GeoDsSrs, width: u32, height: u32,
                                   img_type: GeoDsImgType) -> Result<PathBuf>
    where P: AsRef<Path>
{
    let mut fname = String::with_capacity(256);
    for path in ds_files {
        let src = fs::basename( path).ok_or( invalid_filename(path))?;
        if !fname.is_empty() { fname.push(','); }
        fname.push_str( src);
    }

    let suffix = get_wh_filename( srs.epsg(), bbox, width, height, img_type.file_extension());
    fname.push('_');
    fname.push_str( &suffix);

    let fpath = CACHE_DIR.join(fname);
    Ok( fpath )
}

pub fn get_wh_filename (epsg: u32, bbox: &BoundingBox<f64>, width: u32, height: u32, file_ext: &str) -> String {
    format!("{epsg}_{},{},{},{}_{width}x{height}.{file_ext}",  bbox.west, bbox.south, bbox.east, bbox.north)
}
