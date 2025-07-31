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

use tokio;
use anyhow::Result;
use reqwest::Client;
use clap::Parser;
use odin_n5::{load_config, get_data, Data, N5Config};


#[derive(Parser, Debug)]
#[command(version, about, long_about = "retrieve data for list of N5 devices")]
pub struct Args {
    #[arg(short,long, default_value_t = 1)]
    pub n_last: usize,

    #[arg(num_args=1..)]
    pub device_ids: Vec<u32>
}

#[tokio::main]
async fn main()->Result<()> {
    odin_build::set_bin_context!();

    let args = Args::parse();

    let config: N5Config = load_config("n5.ron")?;
    let client = Client::new();

    for device_id in &args.device_ids {
        println!("------- device: {}", device_id);
        let data = get_data( &client, &config, *device_id, args.n_last).await?;
        println!("{data:#?}");
    }

    Ok(())
}