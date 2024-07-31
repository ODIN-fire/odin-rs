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

//! tool to read VIIRS hotspots from a local csv file

use anyhow::{Result,anyhow};
use std::path::PathBuf;

use odin_common::define_cli;
use odin_jpss::read_jpss;

define_cli! { ARGS [about="tool to extract hotspots from JPSS VIIRS data product files"] =
    pathname: String [help="path to csv file"]
}

fn main() {
    let path = PathBuf::from( &ARGS.pathname);
    if path.is_file() { 
        let hs = read_jpss(&path).unwrap();
        println!("{}", hs.to_json_pretty().unwrap());
    } else { 
        println!("file not found") }
}
