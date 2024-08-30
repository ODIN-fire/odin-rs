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
use serde::{Serialize, Deserialize};
use std::time::Duration;
use std::sync::Arc;
use odin_common::geo::LatLon;
use odin_common::fs::remove_old_files;
use odin_actor::prelude::*;
use odin_actor::ObjSafeFuture;
use crate::*;
use crate::actor::JpssImportActorMsg;

 #[derive(Serialize,Deserialize,Debug,Clone)]
 pub struct LiveJpssImporterConfig {
     pub server: String,
     pub map_key: String,
     pub satellite: u32,
     pub source: String,
     pub region: Vec<LatLon>,
     pub history: Duration,
     pub request_delay: Vec<Duration>,
     pub max_scan_angle: f64,
     pub max_age: Duration,
     pub cleanup_interval: Duration
 }

 #[derive(Debug)]
pub struct LiveJpssImporter {
    config: LiveJpssImporterConfig,
    cache_dir: Arc<PathBuf>,

    /// values set during initialization
    file_import_task: Option<AbortHandle>,
    overpass_import_task: Option<AbortHandle>,
    file_cleanup_task: Option<AbortHandle>,
}

impl LiveJpssImporter {
    pub fn new (config: LiveJpssImporterConfig) -> Self {
        let cache_dir = Arc::new( odin_build::cache_dir().join("jpss"));
        ensure_writable_dir(cache_dir.as_ref()).unwrap(); // Ok to panic - this is a toplevel application object
        LiveJpssImporter{ config, cache_dir, file_import_task:None, overpass_import_task: None, file_cleanup_task:None }
    }

    fn initialize (&mut self, hself: ActorHandle<JpssImportActorMsg>, orbit_handle: ActorHandle<OrbitActorMsg>) -> Result<()> {
        let config = &self.config;
        let cache_dir = &self.cache_dir;
        self.overpass_import_task = Some( self.spawn_overpass_import_task( hself.clone(), orbit_handle, cache_dir.clone() )? );
        self.file_cleanup_task = Some( self.spawn_file_cleanup_task()? );
        Ok(())
    }

    fn spawn_overpass_import_task (&mut self, hself: ActorHandle<JpssImportActorMsg>, orbit_handle: ActorHandle<OrbitActorMsg>, cache_dir:Arc<PathBuf>) -> Result<AbortHandle> {
        let config = self.config.clone();
        Ok( spawn( &format!("jpss-{}-overpass-acquisition", self.config.satellite), async move {
            run_overpass_acquisition( hself, orbit_handle, config, cache_dir ).await
        })?.abort_handle()
        )
    }

    fn spawn_file_cleanup_task(&mut self)-> Result<AbortHandle> {
        let cache_dir = self.cache_dir.clone();
        let cleanup_interval = self.config.cleanup_interval;
        let max_age = self.config.max_age;

        Ok( spawn( &format!("jpss-{}-file-cleanup-", self.config.satellite), async move {
                run_file_cleanup( cache_dir, cleanup_interval, max_age).await
            })?.abort_handle()
        )
    }

}

impl JpssImporter for LiveJpssImporter {
    fn start (&mut self, hself: ActorHandle<JpssImportActorMsg>, orbit_handle: ActorHandle<OrbitActorMsg>) -> Result<()> {
        self.initialize(hself, orbit_handle)?;
        Ok(())
    }

    fn terminate (&mut self) {
        if let Some(task) = &self.file_import_task { task.abort() }
        if let Some(task) = &self.overpass_import_task { task.abort() }
        if let Some(task) = &self.file_cleanup_task { task.abort() }
    }
}

pub fn get_overpass_request(config: LiveJpssImporterConfig, hself: ActorHandle<JpssImportActorMsg>) -> OverpassRequest {
    OverpassRequest{sat_id: config.satellite,
        scan_angle: config.max_scan_angle,
        history: config.history,
        region: config.region,
        requester: hself.into() }
}

async fn run_init_data_acquisition (hself: ActorHandle<JpssImportActorMsg>, config: LiveJpssImporterConfig, cache_dir:Arc<PathBuf>) -> Result<()> {
    let query_bounds = get_query_bounds(&config.region);
    let url = format!("{}/usfs/api/area/csv/{}/{}/{}/3", &config.server, &config.map_key, &config.source, &query_bounds);
    let filename = get_latest_jpss(&query_bounds, &cache_dir, &url, &config.source).await?;
    let hs = read_jpss(&filename)?;
    hself.try_send_msg(UpdateRawHotspots(hs))?;
    Ok(())
   
}

async fn run_data_acquisition (hself: ActorHandle<JpssImportActorMsg>, config: LiveJpssImporterConfig, cache_dir:Arc<PathBuf>, overpass: OrbitalTrajectory) -> Result<()> {
    // set up schedule for next acquisition 
    let mut schedule = get_data_request_schedule(overpass, config.request_delay)?;
    let query_bounds = get_query_bounds(&config.region);
    for dt_next in schedule.into_iter() {
        //  sleep until next download
        let sleep_time =  Utc::now() - dt_next;
        sleep( sleep_time.to_std()?).await;
        //  download
        let url = format!("{}/usfs/api/area/csv/{}/{}/{}/1", &config.server, &config.map_key, &config.source, &query_bounds);
        let filename = get_latest_jpss(&query_bounds, &cache_dir, &url, &config.source).await?;
        let hs = read_jpss(&filename)?;
        hself.try_send_msg(UpdateRawHotspots(hs))?;
    }
    Ok(())
}


async fn run_overpass_acquisition (hself: ActorHandle<JpssImportActorMsg>, orbit_handle: ActorHandle<OrbitActorMsg>, config: LiveJpssImporterConfig, cache_dir:Arc<PathBuf>) -> Result<()> {
    let sat_id = &config.satellite.clone();
    let hself_id = hself.id.clone();
    // initial overpass download
    let mut last_overpass_date = Utc::now();
    match timeout_query_ref(&orbit_handle, AskOverpassRequest(get_overpass_request(config.clone(), hself.clone())), secs(60)).await {
        Ok(response) => { 
            // switch these two lines back to avoid clone
            hself.try_send_msg(response.clone())?;
            last_overpass_date = response.0.get_end()?; // causes error and exits thread if empty set of overpasses
        }, // send overpasses 
        Err(e) => match e {
            OdinActorError::ReceiverClosed => error!("{} : Orbit Actor not available", hself.id.clone()),
            other => error!("{} : Orbit Actor Error", hself.id.clone())
        }
    }
    // initial data download
    run_init_data_acquisition(hself.clone(), config.clone(), cache_dir.clone()).await?;
    // run update loop
    let mut dt_next = last_overpass_date;
    loop {
        let mut overpass_list = OverpassList::new();
        let hself_id_clone = hself_id.clone();
        // get last overpass datetime - need overpass list for this?
        let sleep_time =  Utc::now() - dt_next;
        // sleep until before last op dt
        sleep( sleep_time.to_std()?).await;
        // request new overpasses 
        match timeout_query_ref(&orbit_handle, AskOverpassRequest(get_overpass_request(config.clone(), hself.clone())), secs(1)).await { // potential issue here with hself not being this object
            Ok(response) => { 
                // get time until next update
                dt_next = response.0.get_end()?;
                overpass_list = response.0.clone();
                // send overpasses 
                hself.try_send_msg(response)?
            }, 
            Err(e) => match e {
                OdinActorError::ReceiverClosed => error!("{} : Orbit Actor not available", hself_id_clone),
                other => error!("{} : Orbit Actor Error", hself_id_clone)
            }
        }
        for overpass in overpass_list.overpasses.into_iter() {
            let cache_dir_clone = cache_dir.clone();
            let config_clone = config.clone();
            let hself_clone = hself.clone();
            run_data_acquisition( hself_clone, config_clone,  cache_dir_clone, overpass).await?;
            // spawn( &format!("jpss-{}-{}-data-acquisition", sat_id.clone(), overpass.t_end.clone()), async move {
            //     run_data_acquisition( hself_clone, config_clone,  cache_dir_clone, overpass).await
            // })?;
        }
    }
    Ok(())
}

async fn run_file_cleanup (cache_dir: Arc<PathBuf>, interval: Duration, max_age: Duration) {
    loop {
        remove_old_files( &cache_dir.as_path(), max_age);
        sleep(interval).await; 
    }
}

fn get_data_request_schedule (overpass: OrbitalTrajectory, request_delays: Vec<Duration>) -> Result<Vec<DateTime<Utc>>> {
    let mut schedule = Vec::new();
    for delay in request_delays.into_iter() {
        let d = overpass.t_end + delay;
        if d > Utc::now() {
            schedule.push(d)
        }
    }
    Ok(schedule)
}
pub struct LiveOrbitCalculator { 
}

 impl OrbitCalculator for LiveOrbitCalculator {
    // add Johann's orbit calculation here
    fn calc_overpass_list (&mut self, overpass_request: &OverpassRequest ) -> Result<OverpassList> {
        Ok(OverpassList::new())
    }
 }