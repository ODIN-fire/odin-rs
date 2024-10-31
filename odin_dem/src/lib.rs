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

pub mod errors;

use std::error::Error;
use std::fs::File;
use std::path::{Path, PathBuf};
use axum::http::{header, HeaderMap, HeaderValue};
use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;
use axum::body::Body;
use errors::op_failed;
use tokio::io;
use lazy_static::lazy_static;
use odin_common::fs;
use odin_gdal::{create_file_from_vrt, get_driver_name_for_extension};
use odin_common::{geo::BoundingBox,net::mime_type_for_extension};
use odin_build::define_load_config;

use crate::errors::OdinDemError;

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

    pub fn file_extension(&self) -> &'static str {
        match *self {
            DemImgType::PNG => "png",
            DemImgType::TIF => "tif",
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
    pub fn from_epsg (epsg: u32) -> Result<DemSRS> {
        if epsg == 4326 {
            Ok(DemSRS::WGS84)
        } else if (epsg >= 32601 && epsg <= 32660) || (epsg >= 32701 && epsg <= 32760) {
            Ok(DemSRS::UTM{epsg})
        } else {
            Err(OdinDemError::UnsupportedTargetSRS(format!("{}", epsg)))
        }
    }

    pub fn epsg(&self) -> u32 {
        match *self {
            DemSRS::WGS84 => 4326,
            DemSRS::UTM{epsg} => epsg,
        }
    }
}



/* #endregion  supported image types, SRS and data sources **********************************************************/

fn get_dem_filename (src: &str, epsg: u32, bbox: &BoundingBox<f64>, file_ext: &str) -> String {
    format!("DEM-{src}-{epsg}-{}_{}_{}_{}.{file_ext}", bbox.west, bbox.south, bbox.east, bbox.north)
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
pub fn get_dem (bbox: &BoundingBox<f64>, srs: DemSRS, img_type: DemImgType, vrt_file: &str, out_dir: &PathBuf) -> Result<(String,File)> {
    let vrt_path = Path::new(vrt_file);
    vrt_path.try_exists()?;
    // we use the *.vrt filename as the data source
    let data_src = vrt_path.file_name().and_then(|s| s.to_str()).ok_or( op_failed("invalide VRT filename"))?;

    let ext = img_type.file_extension();
    let fname = get_dem_filename( data_src, srs.epsg(), bbox, ext);
    let file_path: PathBuf = out_dir.join( fname.as_str());

    let file = if !file_path.exists() {
        odin_gdal::create_file_from_vrt( bbox, srs.epsg(), ext, &DEM_OPTS, &file_path, &vrt_path)?
    } else {
        File::open(&file_path)?
    };

    Ok( (fs::path_to_lossy_string(&file_path),file) )
}
