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
 #[macro_use]
extern crate lazy_static;

use structopt::StructOpt;
use std::{fs::File, io::Write};
use tokio;
use anyhow::{Result, Ok};
use odin_orbital::overpass::{get_tles_celestrak, compute_full_orbits};
use odin_common::{angle::{Latitude, Longitude}, geo::GeoRect};

 /// structopt command line arguments
#[derive(StructOpt,Debug)]
struct CliOpts {
    /// satellite id 
    sat_id: u32,
    /// satellite max scan
    max_scan: f64,
    /// output filename
    filename: String,
}

lazy_static! {
    static ref ARGS: CliOpts = CliOpts::from_args();
}


#[tokio::main]
async fn main() -> Result<()> {
    let region = GeoRect::from_wsen(Longitude::from_degrees(-125.0), Latitude::from_degrees(31.0), Longitude::from_degrees(-104.0), Latitude::from_degrees(50.0));
    let tle = get_tles_celestrak(ARGS.sat_id).await?;
    let overpass = compute_full_orbits(tle, ARGS.max_scan, &region)?;
    let j = serde_json::to_string(&overpass)?;
    let fname = &ARGS.filename;
    let mut file = File::create(fname).expect("Could not create file!");
    file.write(j.as_bytes()).expect("Cannot write to the file!");

    Ok(())
}