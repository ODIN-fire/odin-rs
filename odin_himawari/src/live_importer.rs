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

#[allow(unused)]

use std::{io,fs::File,path::{Path,PathBuf},sync::Arc, time::Duration};
use chrono::{DateTime,Utc,NaiveDate,NaiveTime,NaiveDateTime};
use odin_common::fs::remove_old_files;
use odin_actor::prelude::*;
use crate::{
    HimawariConfig, HimawariHotspot, HimawariHotspotSet, PKG_CACHE_DIR,
    actor::{HimawariHotspotActor, HimawariHotspotActorMsg, HimawariHotspotImporter, Initialize, Update},
    download_hotspots,
    errors::{OdinHimawariError, Result, op_failed}
};

/// an importer for Himawari hotspots that retrieves respective files in realtime
/// from the JAXA ftp server - see <https://www.eorc.jaxa.jp/ptree/userguide.html>
///
/// This implementation is based on the assumption that remote files are not modified
/// after creation but latency cannot be deterministically detected. This leaves us with
/// regular directory polling and file retrieval in case a remote file has not been downloaded yet.
///
/// Note that we cannot rely on a configured or computed schedule to download files since
/// latency is too irregular. According to the documentation the data product is computed
/// every 10 min (starting at full hour) and on average latency of file availability is about
/// 40min (based on timestamp) but in reality this can vary by +/- 10min and sometimes files
/// are even produced out of order (i.e. seemingly skipping a step). This indicates a processing
/// pipeline that is not predictable enough to support a schedule and requires polling.
/// 5 min polling interval (for remote directory entries, not files) seems like a suitable choice
pub struct LiveHimawariHotspotImporter {
    config: Arc<HimawariConfig>,
    cache_dir: Arc<PathBuf>,

    /// values set during initialization
    import_task: Option<AbortHandle>,
    file_cleanup_task: Option<AbortHandle>,
}

impl LiveHimawariHotspotImporter {
    pub fn new (config: Arc<HimawariConfig>, cache_dir: Arc<PathBuf>) -> Self {
        LiveHimawariHotspotImporter{ config, cache_dir, import_task:None, file_cleanup_task:None }
    }

    async fn initialize  (&mut self, hself: ActorHandle<HimawariHotspotActorMsg>) -> Result<()> {
        let config = &self.config;

        self.import_task = Some( self.spawn_import_task( hself)? );
        self.file_cleanup_task = Some( self.spawn_file_cleanup_task()? );
        Ok(())
    }

    fn spawn_import_task(&self, hself: ActorHandle<HimawariHotspotActorMsg>) -> Result<AbortHandle> {
        let config = self.config.clone();
        let cache_dir = self.cache_dir.clone();

        Ok( spawn( &format!("himawari-{}-data-acquisition", self.config.sat_id), async move {
                Self::run_data_acquisition( config, cache_dir, hself).await
            })?.abort_handle()
        )
    }

    async fn run_data_acquisition(config: Arc<HimawariConfig>, cache_dir: Arc<PathBuf>, hself: ActorHandle<HimawariHotspotActorMsg>)->Result<()> {
        //--- retrieve initial data set
        let init_files = download_hotspots(config.as_ref(), Utc::now(), config.init_hours, cache_dir.as_ref(), false).await?;
        let mut hotspots: Vec<HimawariHotspotSet> = init_files.iter().filter_map( |path| HimawariHotspotSet::from_file( config.sat_id, path).ok()).collect();
        Self::fill_in_position_heights( config.as_ref(), &mut hotspots).await?;
        hself.send_msg( Initialize(hotspots)).await?;

        //--- loop to update until this task is shut down
        loop {
            sleep( config.update_interval).await;

            // retrieve all items for this and previous hour that we don't have yet
            let update_files = download_hotspots(config.as_ref(), Utc::now(), 2, cache_dir.as_ref(), true).await?;
            let mut hotspots: Vec<HimawariHotspotSet> = update_files.iter().filter_map( |path| HimawariHotspotSet::from_file( config.sat_id, path).ok()).collect();
            Self::fill_in_position_heights( config.as_ref(), &mut hotspots).await?;

            for hs in hotspots.into_iter() {
                hself.send_msg( Update(hs)).await?;
            }
        }
    }

    async fn fill_in_position_heights (config: &HimawariConfig, hotspots: &mut Vec<HimawariHotspotSet>)->Result<()> {
        if let Some(dem) = config.dem.as_ref() {
            for hs in hotspots.iter_mut() {
                hs.fill_in_position_heights( dem).await?;
            }
        }
        Ok(())
    }

    fn spawn_file_cleanup_task(&mut self)-> Result<AbortHandle> {
        let config = self.config.clone();
        let cache_dir = self.cache_dir.clone();

        Ok( spawn( &format!("himawari-{}-file-cleanup", self.config.sat_id), async move {
                Self::run_file_cleanup( config, cache_dir).await
            })?.abort_handle()
        )
    }

    async fn run_file_cleanup(config: Arc<HimawariConfig>, cache_dir: Arc<PathBuf>) {
        loop {
            remove_old_files( &cache_dir.as_path(), config.max_age);
            sleep( config.cleanup_interval).await;
        }
    }
}

impl HimawariHotspotImporter for LiveHimawariHotspotImporter {
    async fn start (&mut self, hself: ActorHandle<HimawariHotspotActorMsg>) -> Result<()> {
        self.initialize(hself).await?;
        Ok(())
    }

    fn terminate (&mut self) {
        if let Some(task) = &self.import_task { task.abort() }
        if let Some(task) = &self.file_cleanup_task { task.abort() }
    }
}
