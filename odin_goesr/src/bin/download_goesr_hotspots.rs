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

//! this application serves both as a test for GOES-R hotspot data download functions and for configs

use anyhow::Result;
use std::{time::{Duration,Instant},sync::Arc};
use tokio::{self,time::sleep};
use chrono::Utc;

use odin_config::prelude::*;
use odin_common::{define_cli,fs::ensure_writable_dir};
use odin_goesr::*;

use_config!();

define_cli! { ARGS [about="GOES-R file download tool"] =
    config: String [help="pathname to LiveGoesRDataImporterConfig config"]
}

#[tokio::main]
async fn main()->Result<()> {
    let config: LiveGoesRDataImporterConfig = config_for!( &ARGS.config)?;
    let data_dir = odin_config::app_metadata().data_dir.join("goesr");
    ensure_writable_dir(&data_dir)?;

    let client = create_s3_client( config.s3_region.clone()).await?;

    let bucket = &config.bucket;
    let sat_id = config.satellite;
    let source = Arc::new(config.source.clone());
    let n_objs = config.init_records;

    println!("retrieving GOES-{} datasets for product {}\n(terminate with Ctrl-C)", sat_id, source);

    //--- initial download
    println!("\n--- initial download of {} objects started at {}", n_objs, Utc::now());
    let init_objs = get_inital_objects( &client, Utc::now(), bucket, &source, n_objs).await?;
    let mut last_key: Option<String> = init_objs.last().and_then( |o| o.key.clone());;
    for obj in init_objs {
        let gdata = get_goesr_data( &client, obj, &data_dir, bucket, source.clone(), sat_id).await?;
        println!("downloaded initial dataset {:?}", gdata.file);
    }

    //--- periodic updates
    println!("\n--- update download loop started with interval {:?}", config.polling_interval);
    let mut t_last = Instant::now();
    loop {
        let sleep_dur = config.polling_interval - (Instant::now() - t_last);
        println!("...sleeping for {:?} at {}", sleep_dur, Utc::now());
        sleep( sleep_dur ).await;
        t_last = Instant::now();

        let dt = Utc::now();
        match get_most_recent_object( &client, dt, bucket, &source, last_key.as_ref()).await? {
            Some(obj) => {
                let last_key = obj.key.clone();
                let gdata = get_goesr_data( &client, obj, &data_dir, bucket, source.clone(), sat_id).await?;
                println!("downloaded update dataset  {:?}", gdata.file);
            }
            None => println!("WARN: failed to retrieve update")
        }
    }

    Ok(())
}
