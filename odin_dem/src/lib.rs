/*
 * Copyright © 2024, United States Government, as represented by the Administrator of 
 * the National Aeronautics and Space Administration. All rights reserved.
 *
 * The “ODIN” software is licensed under the Apache License, Version 2.0 (the "License"); 
 * you may not use this file except in compliance with the License. You may obtain a copy 
 * of the License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software distributed under
 * the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND,
 * either express or implied. See the License for the specific language governing permissions
 * and limitations under the License.
 */
#![allow(unused)]

use std::{error::Error, ops::Deref, path::{Path, PathBuf}, fs::File, sync::LazyLock};
use axum::{http::{header::CONTENT_TYPE, HeaderMap, HeaderValue}, response::IntoResponse, body::Body};
use errors::{invalid_filename, op_failed};
use serde::{Deserialize, Serialize};
use tokio::io;
use odin_gdal::{create_wh_image_from_vrt, csl_string_list, get_driver_name_for_extension, CslStringList};
use odin_common::{fs, BoundingBox,net::mime_type_for_extension};
use odin_build::define_load_config;

pub mod errors;
use errors::OdinDemError;

type Result<T> = std::result::Result<T, OdinDemError>;

define_load_config!{}

/* #region supported image types, SRS and data sources ******************************************************************/

/// the image types that can be returned by odin_dem
pub enum DemImgType {
    PNG,
    TIF,
}

impl DemImgType {
    pub fn for_ext (file_ext: &str) -> Option<DemImgType> {
        match file_ext {
            "tif" => Some(DemImgType::TIF),
            "png" => Some(DemImgType::PNG),
            _ => None
        }
    }

    pub fn for_mime_type (mime_type: &str) -> Option<DemImgType> {
        match mime_type {
            "image/tif" => Some(DemImgType::TIF),
            "image/png" => Some(DemImgType::PNG),
            _ => None
        }
    }

    pub fn file_extension(&self) -> &'static str {
        match *self {
            DemImgType::PNG => "png",
            DemImgType::TIF => "tif",
        }
    }

    // unfortunately we can't do this as a static ref since CslStringList has mutable fields
    pub fn gdal_create_options(&self) -> Option<CslStringList> {
        match *self {
            DemImgType::PNG => None,
            DemImgType::TIF => Some( csl_string_list!("COMPRESS=DEFLATE", "PREDICTOR=2") )
        }
    }

    pub fn gdal_driver_name(&self) -> &'static str {
        get_driver_name_for_extension( self.file_extension()).expect("unsupported GDAL image type")
    }

    pub fn content_type(&self) -> &'static str {
        mime_type_for_extension( &self.file_extension()).expect("unknown mime type")
    }
}

/// the spatial reference systems odin_dem can convert to
pub enum DemSRS {
    WGS84,
    UTM { epsg: u32 },
}

impl DemSRS {
    pub fn from_epsg (epsg: u32) -> Option<DemSRS> {
        match epsg {
            4326 => Some(DemSRS::WGS84),
            32601..32661 => Some(DemSRS::UTM{epsg}),
            32701..32761 => Some(DemSRS::UTM{epsg}),
            _ => None
        }
    }

    pub fn from_srs_spec (srs: &str) -> Option<DemSRS> {
        if srs.starts_with("epsg:") || srs.starts_with("EPSG:") {
            if let Ok(epsg) = srs[5..].parse::<u32>() {
                return Self::from_epsg( epsg)
            }
        } else {
            if let Ok(epsg) = srs.parse::<u32>() {
                return Self::from_epsg( epsg)
            } else {
                if srs.eq_ignore_ascii_case("WGS84") {
                    return Some(DemSRS::WGS84)
                }
            }

            // TODO - support more specs
        }

        None
    }

    pub fn epsg(&self) -> u32 {
        match *self {
            DemSRS::WGS84 => 4326,
            DemSRS::UTM{epsg} => epsg,
        }
    }
}

#[derive(Debug,Serialize,Deserialize,Clone)]
pub struct Resolution {
    pub width: u64,
    pub height: u64
}

/* #endregion  supported image types, SRS and data sources **********************************************************/

fn get_wh_dem_filename (src: &str, epsg: u32, bbox: &BoundingBox<f64>, width: u32, height: u32, file_ext: &str) -> String {
    format!("{src}_{epsg}_{},{},{},{}_{width}x{height}.{file_ext}",  bbox.west, bbox.south, bbox.east, bbox.north)
}

fn get_res_dem_filename (src: &str, epsg: u32, bbox: &BoundingBox<f64>, res_x: f64, res_y: f64, file_ext: &str) -> String {
    format!("{src}_{epsg}_{},{},{},{}_{res_x},{res_y}.{file_ext}",  bbox.west, bbox.south, bbox.east, bbox.north)
}

/// HTTP response creation
async fn create_response (file: File, img_type: DemImgType) -> impl IntoResponse {
    let f = tokio::fs::File::from_std(file);
    let stream = tokio_util::io::ReaderStream::new(f);
    let body = Body::from_stream(stream);

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(img_type.content_type()));

    (headers,body)
}

pub fn dem_cache_dir()->PathBuf {
    let path = odin_build::cache_dir().join("odin_dem");
    fs::ensure_dir(&path).expect( &format!("unable to create DEM cache dir at {:?}", path));
    path
}

//--- main lib entry

const DEM_OPTS: &[&'static str] = &[ "COMPRESS=DEFLATE", "PREDICTOR=2"];

/// for a given bounding box 'bbox' look for a matching file in 'cache_dir'.
/// If none found yet create a file with the given 'img_type' from the virtual GDAL tileset specified by 'vrt_file'
/// note that `bbox` has to be in `srs` units (degree for GEO, meters for UTM)
pub fn get_wh_dem<P> (bbox: &BoundingBox<f64>, srs: DemSRS, width: u32, height: u32, img_type: DemImgType, vrt_file: &P, out_dir: &PathBuf) -> Result<PathBuf> 
    where P: AsRef<Path>
{
    let vrt_path = vrt_file.as_ref();
    vrt_path.try_exists()?;
    let data_src = fs::basename(vrt_file).ok_or(invalid_filename( format!("{:?}", vrt_path)))?;
    let ext = img_type.file_extension();
    let create_opts = img_type.gdal_create_options();
    let fname = get_wh_dem_filename( data_src, srs.epsg(), bbox, width, height, ext);
    let file_path: PathBuf = out_dir.join( fname.as_str());

    if !file_path.exists() {
        odin_gdal::create_wh_image_from_vrt( bbox, srs.epsg(), width, height, ext, &create_opts, &file_path, &vrt_path)?;
    } else {
        fs::set_accessed(&file_path)?; // update atime so that we could use it for LRU cache bounds
    }

    Ok( file_path )
}

pub fn get_res_dem<P> (bbox: &BoundingBox<f64>, srs: DemSRS, res_x: f64, res_y: f64, img_type: DemImgType, vrt_file: &P, out_dir: &PathBuf) -> Result<PathBuf> 
    where P: AsRef<Path>
{
    let vrt_path = vrt_file.as_ref();
    vrt_path.try_exists()?;
    let data_src = fs::basename(vrt_file).ok_or(invalid_filename( format!("{:?}", vrt_path)))?;
    let ext = img_type.file_extension();
    let create_opts = img_type.gdal_create_options();
    let fname = get_res_dem_filename( data_src, srs.epsg(), bbox, res_x, res_y, ext);
    let file_path: PathBuf = out_dir.join( fname.as_str());

    if !file_path.exists() {
        odin_gdal::create_res_image_from_vrt( bbox, srs.epsg(), res_x, res_y, ext, &create_opts, &file_path, &vrt_path)?;
    } else {
        fs::set_accessed(&file_path)?; // update atime so that we could use it for LRU cache bounds
    }

    Ok( file_path )
}