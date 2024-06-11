/*
 * Copyright (c) 2024, United States Government, as represented by the
 * Administrator of the National Aeronautics and Space Administration.
 * All rights reserved.
 *
 * The ODIN - Open Data Integration Framework is licensed under the
 * Apache License, Version 2.0 (the "License"); you may not use this file
 * except in compliance with the License. You may obtain a copy of the
 * License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */
#![allow(unused)]

//! tool to read GOES-R hotspots from a local netcdf file

use anyhow::{Result,anyhow};
use std::{sync::Arc,path::Path};

use odin_common::define_cli;
use odin_goesr::{parse_filename, read_goesr_data, GoesRData, GoesrFileInfo};

define_cli! { ARGS [about="tool to extract hotspots from GOES-R OR_ABI-L2-FDCC data product files"] =
    pathname: String [help="path to netcdf file"]
}

fn main() {
    let path = Path::new( &ARGS.pathname);
    if path.is_file() { 
        let filename = path.file_name().unwrap();
        if let Some(file_info) = parse_filename(filename) {
            let gdata = GoesRData {
                sat_id: file_info.sat_id,
                file: path.to_path_buf(),
                source: Arc::new(format!("{}-{}-{}", file_info.instrument, file_info.level, file_info.product)),
                date: file_info.create_time
            };

            let hs = read_goesr_data( &gdata).unwrap();
            println!("{}", hs.to_json_pretty().unwrap());

        } else { println!("not a valid GOES-R filename") }
    } else { println!("file not found") }
}
