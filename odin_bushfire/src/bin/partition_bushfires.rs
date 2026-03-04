/*
 * Copyright © 2026, United States Government, as represented by the Administrator of
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

use std::{path::{Path,PathBuf},time::Duration};
use chrono::{DateTime,Utc};
use anyhow::{anyhow,Result};

use odin_common::{define_cli, json_writer::{JsonWriter,JsonWritable}, datetime::days};
use odin_bushfire::{
    CACHE_DIR, Bushfire, get_bushfires, get_features, cleanup_feature_properties
};

define_cli! { ARGS [about="grid Basic ECMWF-IFS JSON file"] =
    output_dir: Option<String> [help="directory where to store fire files", long],
    max_days: u64 [help="max age of fire updates in days", long,short, default_value="30"],
    input_file: String [help="filename of JSON input file"],
}

fn main ()->Result<()> {
    let dir = if let Some(dir) = &ARGS.output_dir {
        Path::new(dir).to_path_buf()
    } else {
        CACHE_DIR.clone()
    };
    let max_age: Duration = days( ARGS.max_days);

    let path = Path::new(&ARGS.input_file);
    if path.is_file() {
        let mut features = get_features(&path)?;
        cleanup_feature_properties(&mut features);

        let bushfires = get_bushfires( &features, Some(&dir), Some(max_age))?;

        let mut w = JsonWriter::with_capacity(1024);
        for bf in &bushfires {
            bf.write_json_to( &mut w);
            println!("{}", w.as_str());
            w.clear();
        }

        Ok(())

    } else {
        Err( anyhow!("no input file"))
    }
}
