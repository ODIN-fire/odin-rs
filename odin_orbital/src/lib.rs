/*
 * Copyright © 2025, United States Government, as represented by the Administrator of 
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

use nalgebra::{ViewStorage,base::{Matrix,ArrayStorage,dimension::{Const,Dyn}}};
use chrono::{DateTime,Utc,TimeZone};
use satkit::{Instant,Duration,frametransform::qteme2itrf};
use serde::{Deserialize,Serialize};
use odin_build::{define_load_config,define_load_asset};
use odin_common::{angle::Angle90, cartesian3::Cartesian3, cartographic::Cartographic, datetime};
use odin_macro::public_struct;

pub mod errors;
use errors::{OdinOrbitalError,Result,op_failed};

pub mod orbitinfo;
pub mod overpass;
pub mod tle_store;
pub mod live_firms_importer;

define_load_config!{}
define_load_asset!{}

/// the general information about an orbital satellite 
#[derive(Debug,Clone,Serialize,Deserialize)]
#[public_struct]
pub struct SatelliteInfo {
    sat_id: u32,
    name: String,
    instrument: String,
    max_scan_angle: Angle90,
}

//--- general utility functions

pub fn instant_from_datetime<Z> (dt: DateTime<Z>)->Instant where Z:TimeZone {
    Instant::from_unixtime( dt.timestamp_millis() as f64 / 1000.0)
}

pub fn instant_from_datetime_spec (ds: &str) -> Result<Instant> {
    datetime::parse_datetime(ds).ok_or( op_failed!("invalid datetime spec {}", ds)).map( |dt| instant_from_datetime(dt))
}

pub fn get_time_vec (orbit_duration: Duration, time_step: Duration, start_time: Instant)->Vec<Instant> {
    let n = (orbit_duration.as_seconds() / time_step.as_seconds()).ceil() as usize + 5; // the TLE mean_motion is just that - mean
    let mut t = start_time;

    let mut tv: Vec<Instant> = Vec::with_capacity(n);
    for i in 0..n {
        tv.push(t);
        t += time_step;
    }

    tv
}

pub type ColumnVec<'a> = Matrix<f64, Const<3>, Const<1>, ViewStorage<'a, f64, Const<3>, Const<1>, Const<1>, Const<3>>>;

pub fn get_cartographic (t: &Instant, v: &ColumnVec) -> Cartographic {
    let itrf = qteme2itrf( t).to_rotation_matrix() * v;
    let p = Cartesian3::from_col( &itrf);
    Cartographic::from(p)
}