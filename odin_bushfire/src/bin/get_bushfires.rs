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

use std::{path::{Path,PathBuf}};
use anyhow::{anyhow,Result};
use reqwest::Client;
use chrono::Utc;
use tokio;

use odin_common::{define_cli};
use odin_bushfire::{
    load_config, BushFireConfig, Bushfire, CACHE_DIR, download_file, snapshot_path
};

define_cli! { ARGS [about="grid Basic ECMWF-IFS JSON file"] =
    output_dir: Option<String> [help="directory where to store fire files", short, long],
}

#[tokio::main]
async fn main ()->Result<()> {
    let client = Client::new();
    let config: BushFireConfig = load_config("bushfire.ron")?;
    let date = Utc::now();

    match download_file( &client, &config.url, date).await {
        Ok(path) => {
            println!("downloaded bushfire snapshot to: {path:?}");
            Ok(())
        }
        Err(e) => {
            eprintln!("bushfire snapshot download failed: {e}");
            Err(anyhow!(e))
        }
    }
}
