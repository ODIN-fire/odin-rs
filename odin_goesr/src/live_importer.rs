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
use odin_common::fs::ensure_writable_dir;
use std::{path::Path,time::Instant};

#[derive(Serialize,Deserialize,Debug,Clone)]
pub struct LiveGoesRDataImporterConfig {
    pub satellite: u8,  // 16 or 18
    pub s3_region: String, // e.g. "us-east-1"
    pub bucket: String, // e.g. "noaa-goes18"
    pub source: String, // e.g. "ABI-L2-FDCC"
    pub polling_interval: Duration,
    pub keep_files: bool,
    pub init_records: usize, // number of most recent data files to retrieve
    pub cleanup_interval: Duration,
    pub max_age: Duration,
}

#[derive(Debug)]
pub struct LiveGoesRDataImporter {
    config: LiveGoesRDataImporterConfig,
    data_dir: Arc<PathBuf>,
    import_task: Option<AbortHandle>,
    file_cleanup_task: Option<AbortHandle>,
}

impl LiveGoesRDataImporter {
    pub fn new (config: LiveGoesRDataImporterConfig) -> Self {
        let data_dir = Arc::new( odin_config::app_metadata().data_dir.join("goesr"));
        ensure_writable_dir(&data_dir).unwrap(); // Ok to panic - this is a toplevel application object

        LiveGoesRDataImporter{ config, data_dir, import_task:None, file_cleanup_task:None }
    }

    async fn initialize  (&mut self, hself: ActorHandle<GoesRActorMsg>) -> Result<()> { 
        let s3_client = create_s3_client( self.config.s3_region.clone()).await?; // no point spawning tasks if we can't create an s3_client
        self.import_task = Some( self.spawn_import_task( s3_client, hself)? );
        self.file_cleanup_task = Some( self.spawn_file_cleanup_task()? );
        Ok(())
    }

    fn spawn_import_task(&mut self, client: Client, hself: ActorHandle<GoesRActorMsg>) -> Result<AbortHandle> { 
        let data_dir = self.data_dir.clone();
        let config = self.config.clone();

        Ok( spawn( &format!("goes-{}-data-acquisition", self.config.satellite), async move {
                run_data_acquisition( hself, config, data_dir, client).await
            })?.abort_handle()
        )
    }

    fn spawn_file_cleanup_task(&mut self)-> Result<AbortHandle> {
        let data_dir = self.data_dir.clone();
        let cleanup_interval = self.config.cleanup_interval;
        let max_age = self.config.max_age;

        Ok( spawn( &format!("goes-{}-file-cleanup", self.config.satellite), async move {
                run_file_cleanup( data_dir, cleanup_interval, max_age).await
            })?.abort_handle()
        )
    }
}

impl GoesRDataImporter for LiveGoesRDataImporter {
    async fn start (&mut self, hself: ActorHandle<GoesRActorMsg>) -> Result<()> {
        self.initialize(hself).await?;
        Ok(())
    }

    fn terminate (&mut self) {
        if let Some(task) = &self.import_task { task.abort() }
        if let Some(task) = &self.file_cleanup_task { task.abort() }
    }
}

async fn run_data_acquisition ( hself: ActorHandle<GoesRActorMsg>, config: LiveGoesRDataImporterConfig, data_dir: Arc<PathBuf>, client: Client) {
    let source = Arc::new( config.source); // no need to keep gazillions of copies
    let bucket = &config.bucket;
    let sat_id = config.satellite;
    let mut last_key: Option<String> = None;

    match initial_download( &client, bucket, source.clone(), sat_id, config.init_records, &data_dir).await {
        Ok( (key,hotspots) ) => {
            last_key = key;
            hself.send_msg( Initialize(hotspots) ).await;
        }
        Err(e) => {
            //error!("failed to download initial goes-{} data: {:?}", config.satellite, e);
            hself.try_send_msg( ImportError(e));
        }
    }
    let mut t_last = Instant::now();

    loop {
        sleep( config.polling_interval - (Instant::now() - t_last) ).await; // sleep for remainder of polling interval

        match update_download( &client, bucket, source.clone(), sat_id, &data_dir, last_key.as_ref()).await {
            Ok( (key,hs)) => {
                last_key = key;
                hself.send_msg( Update(hs)).await;
            }
            Err(e) => {
                //error!("failed to download goes-{} update data: {:?}", config.satellite, e);
                hself.try_send_msg( ImportError(e));
            }
        }

        t_last = Instant::now();
    }
}

async fn initial_download (client: &Client, bucket: &str, source: Arc<String>, sat_id: u8, n_objs: usize, data_dir: &PathBuf) -> Result<(Option<String>,Vec<GoesRHotSpots>)> {
    let init_objs = get_inital_objects( client, Utc::now(), bucket, &source, n_objs).await?;
    let last_key: Option<String> = init_objs.last().and_then( |o| o.key.clone());
    let mut hotspots: Vec<GoesRHotSpots> = Vec::with_capacity(init_objs.len());

    for obj in init_objs {
        let gdata = get_goesr_data( client, obj, data_dir, bucket, source.clone(), sat_id).await?;
        match read_goesr_data( &gdata) {
            Ok(hs) => hotspots.push(hs),
            Err(e) => warn!("error parsing GOES-R data: {e:?}")
        }
    }

    Ok( (last_key,hotspots) )
}

async fn update_download (client: &Client, bucket: &str, source: Arc<String>, sat_id: u8, data_dir: &PathBuf, last_key: Option<&String>) -> Result<(Option<String>,GoesRHotSpots)> {
    match get_most_recent_object( client, Utc::now(), bucket, &source, last_key).await? {
        Some(obj) => {
            let last_key = obj.key.clone();
            let gdata = get_goesr_data( client, obj, data_dir, bucket, source, sat_id).await?;
            let hs = read_goesr_data( &gdata)?;
            Ok( (last_key,hs) )
        }
        None => Err( OdinGoesRError::NoObjectError(format!("failed to retrieve last object for goes-{}", sat_id)) )
    }
}

async fn run_file_cleanup (data_dir: Arc<PathBuf>, interval: Duration, max_age: Duration) {
    loop {
        remove_old_files( &data_dir.as_path(), max_age);
        sleep(interval).await; // no need to compensate for cycle execution time
    }
}
