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

use crate::*;
use odin_actor::ObjSafeFuture;
use odin_common::fs::ensure_writable_dir;
use odin_common::s3::{create_s3_client, get_s3_objects, get_last_s3_object};
use odin_common::schedule::{get_hourly_schedule,Compaction,get_next_hourly_event_dtg};
use std::{path::Path,time::Instant};

/// configuration for live GoesR FDCC hotspot import
#[derive(Serialize,Deserialize,Debug,Clone)]
pub struct LiveGoesRHotspotImporterConfig {
    pub satellite: u8,  // 16 or 18
    pub s3_region: String, // e.g. "us-east-1"
    pub bucket: String, // e.g. "noaa-goes18"
    pub source: String, // e.g. "ABI-L2-FDCC"
    pub keep_files: bool,
    pub init_files: usize, // number of most recent data files to retrieve
    pub cleanup_interval: Duration,
    pub max_age: Duration,
}

/// the structure representing objects to collect and announce availability of live GoesR FDCC fire product data (hotspots)
/// 
/// (REQ) instance should check availability of new data sets on a guaranteed time interval
/// (REQ) instance should not miss any available data set once initialized 
#[derive(Debug)]
pub struct LiveGoesRHotspotImporter {
    config: LiveGoesRHotspotImporterConfig,
    cache_dir: Arc<PathBuf>,

    /// values set during initialization
    import_task: Option<AbortHandle>,
    file_cleanup_task: Option<AbortHandle>,
}

impl LiveGoesRHotspotImporter {
    pub fn new (config: LiveGoesRHotspotImporterConfig) -> Self {
        let cache_dir = Arc::new( odin_config::app_metadata().cache_dir.join("goesr"));
        ensure_writable_dir(&cache_dir).unwrap(); // Ok to panic - this is a toplevel application object

        LiveGoesRHotspotImporter{ config, cache_dir, import_task:None, file_cleanup_task:None }
    }

    async fn initialize  (&mut self, hself: ActorHandle<GoesRHotspotImportActorMsg>) -> Result<()> { 
        let config = &self.config;
        let init_files = config.init_files;
        let s3_client = create_s3_client( config.s3_region.clone()).await?;

        self.import_task = Some( self.spawn_import_task( s3_client, hself)? );
        self.file_cleanup_task = Some( self.spawn_file_cleanup_task()? );
        Ok(())
    }

    fn spawn_import_task(&mut self, client: S3Client, hself: ActorHandle<GoesRHotspotImportActorMsg>) -> Result<AbortHandle> { 
        let data_dir = self.cache_dir.clone();
        let config = self.config.clone();

        Ok( spawn( &format!("goes-{}-data-acquisition", self.config.satellite), async move {
                run_data_acquisition( hself, config, data_dir, client).await
            })?.abort_handle()
        )
    }

    fn spawn_file_cleanup_task(&mut self)-> Result<AbortHandle> {
        let cache_dir = self.cache_dir.clone();
        let cleanup_interval = self.config.cleanup_interval;
        let max_age = self.config.max_age;

        Ok( spawn( &format!("goes-{}-file-cleanup", self.config.satellite), async move {
                run_file_cleanup( cache_dir, cleanup_interval, max_age).await
            })?.abort_handle()
        )
    }
}

impl GoesRHotspotImporter for LiveGoesRHotspotImporter {
    async fn start (&mut self, hself: ActorHandle<GoesRHotspotImportActorMsg>) -> Result<()> {
        self.initialize(hself).await?;
        Ok(())
    }

    fn terminate (&mut self) {
        if let Some(task) = &self.import_task { task.abort() }
        if let Some(task) = &self.file_cleanup_task { task.abort() }
    }
}

async fn run_data_acquisition (hself: ActorHandle<GoesRHotspotImportActorMsg>, config: LiveGoesRHotspotImporterConfig, cache_dir: Arc<PathBuf>, client: S3Client)->Result<()> 
{
    let source = Arc::new( config.source); // no need to keep gazillions of copies
    let bucket = &config.bucket;
    let sat_id = config.satellite;
    let mut last_obj: Option<S3Object> = None;

    //--- get 3h most recent object entries so that we can build a schedule
    let mut objs = get_most_recent_objects( &client, &config.bucket, &source, Duration::from_hours(3), Utc::now()).await?;
    if objs.len() < 12 { return Err(no_object_error("not enough initial objects")) }

    let hourly_schedule = get_hourly_schedule(&objs, Some(Compaction::BoundedRightEdge(3)));
    let mut init_objs = if objs.len() > config.init_files { objs.split_off( objs.len()-config.init_files) } else { objs };

    //--- now get the initial files and send an Initialize msg with the hotspots read from them
    let hotspots = download_and_read_objects( &client, bucket, &source, sat_id, &cache_dir, &init_objs).await?;
    last_obj = init_objs.pop();
    hself.send_msg( Initialize(hotspots) ).await;

    //--- run update loop
    loop {
        let dt_cycle = Utc::now();
        let dt_next = get_next_hourly_event_dtg( dt_cycle, &hourly_schedule);
        sleep( (dt_next - dt_cycle).to_std()?).await;

        let mut update_objs = get_objects_since( &client, &config.bucket, &source, &last_obj, dt_cycle, Utc::now()).await?;
        // here we could dynamically re-compute/adapt the hourly_schedule if we repeatedly get multiple objects

        let mut hotspots = download_and_read_objects( &client, bucket, &source, sat_id, &cache_dir, &update_objs).await?;
        last_obj = update_objs.pop().or( last_obj);

        for hs in hotspots {
            hself.send_msg( Update(hs)).await?;
        }
    }

    Ok(())
}

async fn run_file_cleanup (cache_dir: Arc<PathBuf>, interval: Duration, max_age: Duration) {
    loop {
        remove_old_files( &cache_dir.as_path(), max_age);
        sleep(interval).await; // no need to compensate for cycle execution time
    }
}
