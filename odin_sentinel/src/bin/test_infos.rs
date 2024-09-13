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
///! simple utility to test SentinelDeviceInfo config files
///! TODO - we should also test image retrieval

use std::path::Path;
use odin_build::load_config_path;
use odin_common::define_cli;
use odin_sentinel::{load_config, SentinelDeviceInfos, SentinelDeviceInfo};
use anyhow::Result;
 
define_cli! { ARGS [about="static SentinelDeviceInfo test"] = 
    pretty: bool            [help="pretty print config", long],
    path: Option<String>    [help="optional pathname of SentinelDeviceInfo config file"]
}

fn main()->Result<()> {
    let dev_infos: SentinelDeviceInfos = if let Some(pathname) = &ARGS.path {
        let path = Path::new(pathname);
        load_config_path( &path)?
    } else {
        load_config("sentinel_info.ron")?
    };

    println!("\n-- Rust");
    if ARGS.pretty {
        println!("{:#?}", dev_infos)
    } else {
        println!("{:?}", dev_infos)
    }

    println!("\n-- JSON");
    let dis: Vec<&SentinelDeviceInfo> = dev_infos.values().collect();
    println!( "{}", if ARGS.pretty { serde_json::to_string_pretty(&dis)? } else { serde_json::to_string(&dis)? });

    Ok(())
}