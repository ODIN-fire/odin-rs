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
use odin_common::{define_serde_struct, fs::EnvPathBuf, geo::GeoPoint};
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


pub async fn get_evt_value_at_geo_point (lf_src: &GeoDs, pos: &GeoPoint)->Result<usize> {
    let srs = GeoDsSrs::from_epsg(4326).unwrap(); // -> locations are lat,lon
    let mut locations = vec![ (pos.latitude_degrees(), pos.longitude_degrees()) ];

    let vs = lf_src.get_values( "EVT", Some(-9999.00), &srs, &mut locations).await?;

    if vs.len() != 1 { return Err( op_failed!("invalid response length")) }
    if vs[0] < 0.0 { return Err( op_failed!("no EVT value for location")) }

    Ok( vs[0] as usize )
}
