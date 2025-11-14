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

use std::{net::SocketAddr,any::type_name, path::{Path,PathBuf}, fs, io::{BufReader, Read}, fs::File, sync::Arc};
use chrono::{DateTime,Utc,NaiveDate};
use regex::Regex;
use serde::{Serialize,Deserialize};

use odin_build::prelude::*;
use odin_macro::public_struct;
use odin_common::{
    datetime::{ser_short_rfc3339, ser_short_rfc3339_opt, ser_epoch_millis}, define_serde_struct, 
    fs::{filepath_contents_as_string, get_filename_extension, matching_files_in_tree, EnvPathBuf}, 
    geo::GeoPoint
};

pub mod fire_service;
pub mod errors;
pub use errors::Result;

define_load_config!{}
define_load_asset!{}

/// specification of where to look up fire summaries
#[derive(Deserialize,Serialize,Debug)]
pub struct FiresConfig {
    pub src_dir: EnvPathBuf,
    pub summary_pattern: String
}

define_serde_struct! {
FireSummaryMsg [rename_all="camelCase"] = 
    fire_summary: FireSummary
}

define_serde_struct! {
#[public_struct]
FireSummary [rename_all="camelCase"] =  
    year: u32,
    name: String,

    unique_id: String,
    irwin_id: String,
    inciweb_id: String,

    start: NaiveDate,
    contained: Option<NaiveDate>,
    end: NaiveDate,

    location: GeoPoint,
    acres: f32,              // TODO - use last perimeter

    ignitions: Vec<Ignition>,
    perimeters: Vec<Perimeter>,

    containment: Vec<Containment>

        //... and many more to come (esp. ops)
}


define_serde_struct! {
#[public_struct]
Ignition =
    name: String,
    datetime: DateTime<Utc>,
    location: GeoPoint,
    cause: String
}

define_serde_struct! {
#[public_struct]
Perimeter =
    datetime: DateTime<Utc>,
    acres: f32, // TODO - use uom aera
    agency: String,
    method: String
}

define_serde_struct! {
#[public_struct]
Containment =
    date: NaiveDate,
    acres: f32,
    percent: u8
}


pub fn default_data_dir()->PathBuf {
    pkg_data_dir!()
}

pub fn load_summaries<P: AsRef<Path>> (dir: P, file_pattern: Regex)->Result<Vec<(PathBuf,FireSummary)>> {
    let summary_paths = matching_files_in_tree( dir, &file_pattern)?;
    let mut list: Vec<(PathBuf,FireSummary)> = Vec::new();

    for p in &summary_paths {
        let file = File::open(p)?;
        match serde_json::from_reader( file) {
            Ok(summary) => {
                list.push( (p.clone(), summary) )
            }
            Err(e) => {
                eprintln!("error parsing file {:?}: {}", p, e);
                return Err( errors::OdinFiresError::SerdeError(e));
            }
        }
    }

    Ok(list)
} 


/*
    "fireSummary": {
        "year": 2018,
        "name": "Camp",
        "uniqueId": "2018-CABTU-016737",
        "irwinId": "{75E64DB8-9B75-4A68-BEDD-67CC62658E38}",
        "inciwebId": "6250",

        "start": "2018-11-08",
        "contained": "2018-11-25",
        "end": "2018-11-25",

        "location": { "lat": 39.810278, "lon": -121.437222 },
        "acres": 153336,

        "ignitions": [
            { "datetime": "2018-11-08T14:33:00Z", "lat": 39.810278, "lon": -121.437222, "name": "Camp Fire", "cause": "power line failure" }
        ],

        "perimeters": [
            { "datetime": "2018-11-08T17:54:00Z",  "acres": 54586.4327952, "agency": "CDF", "method": "Infrared Image" },
            { "datetime": "2018-11-09T00:00:00Z",  "acres": 70454.1003422, "agency": "CDF", "method": "Mixed Methods"  },
            { "datetime": "2018-11-09T19:02:00Z",  "acres": 109502.882411, "agency": "CDF", "method": "Infrared Image" }
        ],

        "containment": [],
        "resources": [],
        "wind": [],
        "structures": [],
        "casualties": []
    }
}
*/