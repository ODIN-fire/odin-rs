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
#![feature(duration_constructors)]

//! this application serves both as a test for GOES-R hotspot data download functions and associated configs,
//! and as a production tool to obtain raw GOES-R hotspot data (e.g. for archives)
//! It is basically the same logic as the live_importer [`run_data_acquisition`] without parsing into
//! GoesRHotspots but with more logging output.

use uom::si::time::hour;
use std::{time::{Duration,Instant},sync::Arc};
use tokio::{self,time::sleep};
use chrono::{DateTime,Utc};

use odin_build;
use odin_common::{define_cli,fs::ensure_writable_dir};
use odin_common::s3::{S3Object,create_s3_client, get_s3_objects, get_last_s3_object};
use odin_common::schedule::{get_hourly_schedule,Compaction,get_next_hourly_event_dtg};
use odin_goesr::{load_config,get_goesr_data, get_most_recent_objects, get_objects_since, no_object_error, OdinGoesRError, Result, LiveGoesRHotspotImporterConfig};

define_cli! { ARGS [about="GOES-R file download tool"] =
    config: String [help="pathname to LiveGoesRDataImporterConfig config"]
}

#[tokio::main]
async fn main()->Result<()> {
    odin_build::set_bin_context!();
    let config: LiveGoesRHotspotImporterConfig = load_config( &ARGS.config)?;
    let cache_dir = odin_build::cache_dir().join("goesr");
    ensure_writable_dir(&cache_dir)?;

    let client = create_s3_client( config.s3_region.clone()).await?;
    let bucket = &config.bucket;
    let sat_id = config.satellite;
    let source = Arc::new(config.source.clone());
    let n_objs = config.init_files;
    let mut last_obj: Option<S3Object> = None;

    println!("retrieving GOES-{} datasets for product {}\n(terminate with Ctrl-C)", sat_id, source);

    //--- initial download
    println!("\n----------- initial download of {} objects started at {}", n_objs, Utc::now());
    let mut objs = get_most_recent_objects( &client, &config.bucket, &source, Duration::from_hours(3), Utc::now()).await?;
    if objs.len() < 12 { return Err(no_object_error("not enough initial objects")) }

    let hourly_schedule = get_hourly_schedule(&objs, Some(Compaction::BoundedRightEdge(3)));
    let mut init_objs = if objs.len() > config.init_files { objs.split_off( objs.len()-config.init_files) } else { objs };

    for obj in &init_objs {
        let gdata = get_goesr_data( &client, obj, &cache_dir, bucket, source.clone(), sat_id).await?;
        println!("downloaded initial dataset {:?}", gdata.file);
    }
    last_obj = init_objs.pop();

    //--- periodic updates
    println!("\nstarting update loop with hourly schedule {:?}", hourly_schedule);
    loop {
        let dt_cycle = Utc::now();
        let dt_next = get_next_hourly_event_dtg( dt_cycle, &hourly_schedule);
        let sleep_dur = (dt_next - dt_cycle).to_std()?;
        println!("----------- {}: next at {} (sleep for {:?})", dt_cycle, dt_next, sleep_dur);
        sleep( sleep_dur).await;

        let mut update_objs = get_objects_since( &client, &config.bucket, &source, &last_obj, dt_cycle, Utc::now()).await?;
        println!("downloading {} objects...", update_objs.len());
        for obj in &update_objs {
            let gdata = get_goesr_data( &client, obj, &cache_dir, bucket, source.clone(), sat_id).await?;
            println!("downloaded update dataset  {:?}", gdata.file);
        }
        last_obj = update_objs.pop().or( last_obj);
    }

    Ok(())
}
