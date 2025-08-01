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

use std::collections::VecDeque;
use uom::si::f64::{Length,Velocity};
use chrono::{DateTime,Utc};
use odin_common::{angle::Angle360, geo::GeoPoint4};
use memchr;

pub mod adsb;
pub mod rs1090;
pub mod sbs;
pub mod errors;

/// the data model for a tracked aircraft
pub struct Aircraft {
    pub icao24: String,
    pub callsign: Option<String>,

    pub pos: VecDeque<GeoPoint4>, // used as a ringbuffer

    pub groundspeed: Option<Velocity>,
    pub vertical_rate: Option<Velocity>,
    pub hdg: Option<Angle360>,

    pub sel_hdg: Option<Angle360>,
    pub sel_alt: Option<Length>,

    pub last_update:  DateTime<Utc>,
    //... and more to follow
}

