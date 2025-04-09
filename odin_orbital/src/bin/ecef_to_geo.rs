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

use std::fs::File;
use chrono::Timelike;
use ron;
use odin_common::cartographic::Cartographic;
use odin_common::fs;
use odin_common::{cartesian3,cartographic,define_cli};
use odin_orbital::overpass::Overpass;

use anyhow::{Result};

define_cli! { ARGS [about="convert ECEF overpass trajectory to geodetic coordinates"] =
    path: String [help="path to overpass file to convert (must be valid RON notation created from Overpass"]
}

fn main ()->Result<()> {
    let data = fs::filepath_contents_as_string( &ARGS.path)?;
    let op: Overpass = ron::from_str(&data)?;
    let mut t = op.start;
    let tstep = op.time_step;

    for (i,p) in op.trajectory.iter().enumerate() {
        let c: Cartographic = p.into();
        println!("[{:3}]: {:02}:{:02}:{:02} = {:10.4} °,{:10.4} °,{:10.0} m", i, 
            t.hour(), t.minute(), t.second(),
            c.longitude_deg(), c.latitude_deg(), c.height
        ); 
        t += tstep;
    }

    Ok(())
}