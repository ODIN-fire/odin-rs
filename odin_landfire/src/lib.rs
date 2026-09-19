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

use std::{path::{PathBuf}, collections::HashMap};
use lazy_static::lazy_static;
use serde::{Deserialize};
use uom::si::f64::{Length};
use uom::si::{length::{meter}};
use odin_build::{pkg_cache_dir, pkg_data_dir, define_load_config, define_load_asset};
use odin_common::{define_serde_struct, fs::EnvPathBuf, geo::GeoPoint, utm::UtmPoint};
use odin_gdal::{self, geods::{GeoDs,GeoDsSrs}};

pub mod evt_cats;

pub mod errors;
use errors::{Result,op_failed, OdinLandfireError};

pub mod service;

lazy_static! {
    pub static ref CACHE_DIR: PathBuf = { pkg_cache_dir!() };
    pub static ref LANDFIRE_PIXEL_RESOLUTION: Length = { Length::new::<meter>(30.0) };
}

/// this can be used for both WMS and ODIN queries. The layer paths do not include file extensions
/// (png for WMS and tif for own)
#[derive(Deserialize,Debug)]
pub struct LandfireConfig {
    pub wms_layers: HashMap<String,EnvPathBuf>,
    pub wms_capabilities: Option<EnvPathBuf>, // only for WMS

    pub tms_layers: HashMap<String,EnvPathBuf>,

    pub color_maps: HashMap<String,EnvPathBuf>,

    pub datasets: HashMap<String,EnvPathBuf>, // for internal queries
}

define_load_config!{}
define_load_asset!{}

const NODATA: f64 = -9999.0;
const NODATA_I16: i16 = -9999;

pub async fn get_evt_value_at_geo_point (lf_src: &GeoDs, pos: &GeoPoint)->Result<i16> {
    let srs = GeoDsSrs::from_epsg(4326).unwrap(); // safe to unwrap as epsg 4326 (lat,lon) is well known
    let mut locations = vec![ (pos.latitude_degrees(), pos.longitude_degrees()) ];

    let vs = lf_src.get_values( "EVT", Some(NODATA), &srs, &mut locations).await?;
    if vs.len() != 1 { return Err( op_failed!("invalid response length")) }

    Ok( vs[0] as i16 )
}

pub async fn get_evt_value_at_utm_point (lf_src: &GeoDs, pos: &UtmPoint)->Result<i16> {
    let srs = GeoDsSrs::from_epsg( pos.utm_zone.epsg()).unwrap(); // safe to unwrap as all valid utm_zones have valid an epsg
    let mut locations = vec![ (pos.easting, pos.northing) ]; // UTM epsg is x,y

    let vs = lf_src.get_values( "EVT", Some(NODATA), &srs, &mut locations).await?;
    if vs.len() != 1 { return Err( op_failed!("invalid response length")) }

    Ok( vs[0] as i16)
}

pub async fn get_evt_values_at_utm_points (lf_src: &GeoDs, pts: &[UtmPoint])->Result<Vec<i16>> {
    if pts.is_empty() {
        Ok( Vec::with_capacity(0) )
    } else {
        let srs = GeoDsSrs::from_epsg( pts[0].utm_zone.epsg()).unwrap(); // safe to unwrap as all valid utm_zones have valid an epsg
        let mut locations: Vec<(f64,f64)> = pts.iter().map( |p| (p.easting, p.northing)).collect();

        let vs = lf_src.get_values( "EVT", Some(NODATA), &srs, &mut locations).await?;
        if vs.len() != locations.len() { return Err( op_failed!("invalid response length")) }
        let vs: Vec<i16> = vs.iter().map( |v| *v as i16).collect();

        Ok( vs )
    }
}

pub fn is_nodata (v: i16)->bool {
    v == NODATA_I16
}
