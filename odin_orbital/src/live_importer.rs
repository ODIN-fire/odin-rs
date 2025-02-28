use chrono::TimeDelta;
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
use std::result;
use std::time::Duration;
use std::sync::Arc;
use odin_common::geo::GeoCoord;
use odin_common::fs::remove_old_files;
use odin_actor::prelude::*;
use crate::*;
use crate::actor::*;

 /* #region configs *************************************************************************************************/

 #[derive(Serialize,Deserialize,Debug,Clone)]

 pub struct LiveOrbitalSatConfig {
    // shared values
    pub satellite: u32,
    pub source: String,
    pub history: Duration,
    pub max_scan_angle: f64,
    pub max_age: Duration,
    pub cleanup_interval: Duration,
 }
 impl LiveOrbitalSatConfig {
    pub fn make_orbital_sat_config(&self) -> OrbitalSatConfig {
        OrbitalSatConfig { satellite: self.satellite.clone(), source: self.source.clone(), max_age: self.max_age.clone() }
    }
 }

 #[derive(Debug,Clone, Deserialize)]
 pub struct LiveOrbitalSatImporterConfig {
    pub server: String,
    pub map_key: String,
    pub satellite: u32,
    pub source: String,
    pub region: GeoRect,
    pub history: Duration,
    pub request_delay: Vec<Duration>,
    pub max_scan_angle: f64,
    pub max_age: Duration,
    pub cleanup_interval: Duration
 }
 impl LiveOrbitalSatImporterConfig {
    pub fn new(live_config: &Arc<LiveOrbitalSatConfig>, importer_config: OrbitalSatImporterConfig) -> Self {
        LiveOrbitalSatImporterConfig {
            server: importer_config.server,
            map_key: importer_config.map_key,
            satellite: live_config.satellite,
            source: live_config.source.clone(),
            region: importer_config.region,
            history: live_config.history,
            request_delay: importer_config.request_delay,
            max_scan_angle: live_config.max_scan_angle,
            max_age: live_config.max_age,
            cleanup_interval: live_config.cleanup_interval 
        }
    } 
 }

 #[derive(Debug,Clone)]
pub struct LiveOrbitalSatOrbitCalculatorConfig {
    pub satellite: u32,
    pub source: String,
    pub full_region: GeoRect,
    pub history: Duration,
    pub calculation_interval: Duration,
    pub max_scan_angle: f64,
    pub max_age: Duration,
    pub cleanup_interval: Duration
}

impl LiveOrbitalSatOrbitCalculatorConfig {
    pub fn new(live_config: &Arc<LiveOrbitalSatConfig>, orbit_config: OrbitalSatOrbitCalculatorConfig) -> Self {
        LiveOrbitalSatOrbitCalculatorConfig {
            satellite: live_config.satellite,
            source: live_config.source.clone(),
            full_region: orbit_config.full_region,
            history: live_config.history,
            calculation_interval: orbit_config.calculation_interval,
            max_scan_angle: live_config.max_scan_angle,
            max_age: live_config.max_age,
            cleanup_interval: live_config.cleanup_interval 
        }
    } 
}   
  /* #endregion configs */


 #[derive(Debug)]
pub struct LiveOrbitalSatImporter {
    config: LiveOrbitalSatImporterConfig,
    cache_dir: Arc<PathBuf>,

    /// values set during initialization
    file_import_task: Option<AbortHandle>,
    overpass_import_task: Option<AbortHandle>,
    file_cleanup_task: Option<AbortHandle>,
}

impl LiveOrbitalSatImporter {
    pub fn new (config: LiveOrbitalSatImporterConfig) -> Self {
        let cache_dir = Arc::new( odin_build::cache_dir().join("orbital"));
        ensure_writable_dir(cache_dir.as_ref()).unwrap(); // Ok to panic - this is a toplevel application object
        LiveOrbitalSatImporter{ config, cache_dir, file_import_task:None, overpass_import_task: None, file_cleanup_task:None }
    }

    fn initialize (&mut self, hself: ActorHandle<OrbitalSatImportActorMsg>, orbit_handle: ActorHandle<OrbitActorMsg>) -> Result<()> {
        let cache_dir = &self.cache_dir;
        println!("initializing");
        self.overpass_import_task = Some( self.spawn_overpass_import_task( hself.clone(), orbit_handle, cache_dir.clone() )? );
        self.file_cleanup_task = Some( self.spawn_file_cleanup_task()? );
        Ok(())
    }

    fn spawn_overpass_import_task (&mut self, hself: ActorHandle<OrbitalSatImportActorMsg>, orbit_handle: ActorHandle<OrbitActorMsg>, cache_dir:Arc<PathBuf>) -> Result<AbortHandle> {
        let config = self.config.clone();
        Ok( spawn( &format!("orbital-{}-overpass-acquisition", self.config.satellite), async move {
            run_overpass_acquisition( hself, orbit_handle, config, cache_dir ).await
        })?.abort_handle()
        )
    }

    fn spawn_file_cleanup_task(&mut self)-> Result<AbortHandle> {
        let cache_dir = self.cache_dir.clone();
        let cleanup_interval = self.config.cleanup_interval;
        let max_age = self.config.max_age;

        Ok( spawn( &format!("orbital-{}-file-cleanup-", self.config.satellite), async move {
                run_file_cleanup( cache_dir, cleanup_interval, max_age).await
            })?.abort_handle()
        )
    }

}

impl OrbitalSatImporter for LiveOrbitalSatImporter {
    fn start (&mut self, hself: ActorHandle<OrbitalSatImportActorMsg>, orbit_handle: ActorHandle<OrbitActorMsg>) -> Result<()> {
        self.initialize(hself, orbit_handle)?;
        Ok(())
    }

    fn terminate (&mut self) {
        if let Some(task) = &self.file_import_task { task.abort() }
        if let Some(task) = &self.overpass_import_task { task.abort() }
        if let Some(task) = &self.file_cleanup_task { task.abort() }
    }
}

pub fn get_overpass_request(config: LiveOrbitalSatImporterConfig, hself: ActorHandle<OrbitalSatImportActorMsg>) -> OverpassRequest {
    OverpassRequest{sat_id: config.satellite,
        scan_angle: config.max_scan_angle,
        history: config.history,
        region: config.region,
        requester: hself.into() }
}

async fn run_init_data_acquisition (hself: ActorHandle<OrbitalSatImportActorMsg>, config: LiveOrbitalSatImporterConfig, cache_dir:Arc<PathBuf>, last_overpass: Overpass) -> Result<()> {
    println!("**running init data acq");
    let query_bounds = get_query_bounds(&config.region);
    let url = format!("{}/usfs/api/area/csv/{}/{}/{}/3", &config.server, &config.map_key, &config.source, &query_bounds); // update date to match history
    let filename = get_latest_hotspot_download( &cache_dir, &url, &config.source).await?;
    let hs = read_hotspots(&filename)?;
    hself.try_send_msg(InitialHotspots(hs))?;
    // run schedule for most recent overpass
    println!("got overpass end date init data acq: {:?}", Utc.timestamp_millis_opt(last_overpass.last_date).unwrap());
    run_data_acquisition( hself, config,  cache_dir, last_overpass).await?;
    Ok(())
   
}

async fn run_data_acquisition (hself: ActorHandle<OrbitalSatImportActorMsg>, config: LiveOrbitalSatImporterConfig, cache_dir:Arc<PathBuf>, overpass: Overpass) -> Result<()> {
    // set up schedule for next acquisition 
    let schedule = get_data_request_schedule(overpass.clone(), config.request_delay)?;
    println!("overpass end date data acq {:?}", Utc.timestamp_millis_opt(overpass.last_date).unwrap());
    println!("schedule: {:?}", schedule);
    
    let query_bounds = get_query_bounds(&config.region);
    for dt_next in schedule.into_iter() {
        //  sleep until next download
        let mut sleep_time = TimeDelta::zero();
        if  (Utc::now() < dt_next) { // protect against zero
            sleep_time =   dt_next - Utc::now();
        }
        println!("sleep time: {:?}", sleep_time.num_minutes());
        sleep( sleep_time.to_std()?).await;
        //  download
        let url = format!("{}/usfs/api/area/csv/{}/{}/{}/1", &config.server, &config.map_key, &config.source, &query_bounds);
        let filename = get_latest_hotspot_download( &cache_dir, &url, &config.source).await?;
        let hs = read_hotspots(&filename)?;
        hself.try_send_msg(UpdateRawHotspots(hs))?;
    }
    Ok(())
}


async fn run_overpass_acquisition (hself: ActorHandle<OrbitalSatImportActorMsg>, orbit_handle: ActorHandle<OrbitActorMsg>, config: LiveOrbitalSatImporterConfig, cache_dir:Arc<PathBuf>) -> Result<()> {
    let hself_id = hself.id.clone();
    // initial overpass download
    let mut last_overpass_date = Utc::now();
    let mut last_overpass = None;
    println!("**running overpass acq");
    match timeout_query_ref(&orbit_handle, AskOverpassRequest(get_overpass_request(config.clone(), hself.clone())), secs(60)).await {
        Ok(response) => { 
            // switch these two lines back to avoid clone
            hself.try_send_msg(response.clone())?; 
            last_overpass_date = response.0.get_next_overpass_end()?; // causes error and exits thread if empty set of overpasses
            last_overpass = response.0.get_most_recent_overpass();
            println!("got overpass end date: {:?}", last_overpass_date);
        }, // send overpasses 
        Err(e) => match e {
            OdinActorError::ReceiverClosed => error!("{} : Orbit Actor not available", hself.id.clone()),
            err => error!("{} : Orbit Actor Error - {}", hself.id.clone(), err)
        }
    }
    // initial data download
    if let Some(overpass) = last_overpass {
        println!("got overpass end date: {:?}", last_overpass_date);
        run_init_data_acquisition(hself.clone(), config.clone(), cache_dir.clone(), overpass).await?;
    } else {
        OdinOrbitalSatError::MiscError(String::from("No overpasses during initial download"));
    }
    // run update loop
    let mut dt_next = last_overpass_date;
    loop {
        let mut overpass_list = OverpassList::new();
        let hself_id_clone = hself_id.clone();
        // get last overpass datetime - need overpass list for this?
        let mut sleep_time = TimeDelta::zero();
        if  (Utc::now() < dt_next) { // protect against negative time
            sleep_time =  dt_next - Utc::now();
        }
        println!("sleep time:{:?}", sleep_time);
        // sleep until before last op dt
        sleep( sleep_time.to_std()?).await;
        // request new overpasses 
        match timeout_query_ref(&orbit_handle, AskOverpassRequest(get_overpass_request(config.clone(), hself.clone())), secs(1)).await { // potential issue here with hself not being this object
            Ok(response) => { 
                // get time until next update
                dt_next = response.0.get_end()?;
                overpass_list = response.0.clone();
                // send overpasses 
                hself.try_send_msg(response)?;
                println!("got new overpasses")
            }, 
            Err(e) => match e {
                OdinActorError::ReceiverClosed => error!("{} : Orbit Actor not available", hself_id_clone),
                err => error!("{} : Orbit Actor Error - {}", hself_id_clone, err)
            }
        }
        for overpass in overpass_list.overpasses.into_iter() {
            let cache_dir_clone = cache_dir.clone();
            let config_clone = config.clone();
            let hself_clone = hself.clone();
            //run_data_acquisition( hself_clone, config_clone,  cache_dir_clone, overpass).await?; 
            spawn( &format!("orbital-{}-{}-data-acquisition", config.satellite.clone(), Utc.timestamp_millis_opt(overpass.last_date.clone()).unwrap()), async move {
                run_data_acquisition( hself_clone, config_clone,  cache_dir_clone, overpass).await 
            })?; // orbits overlap with schcedule therefore we need to spawn
        }
    }
}

async fn run_file_cleanup (cache_dir: Arc<PathBuf>, interval: Duration, max_age: Duration) {
    loop {
        remove_old_files( &cache_dir.as_path(), max_age);
        sleep(interval).await; 
    }
}

fn get_data_request_schedule (overpass: Overpass, request_delays: Vec<Duration>) -> Result<Vec<DateTime<Utc>>> {
    let mut schedule = Vec::new();
    schedule.push( Utc.timestamp_millis_opt(overpass.last_date).unwrap());
    for delay in request_delays.into_iter() {
        let d = Utc.timestamp_millis_opt(overpass.last_date).unwrap() + delay;
        if d > Utc::now() {
            schedule.push(d)
        }
    }
    Ok(schedule)
}

pub struct LiveOrbitCalculator { 
    config: LiveOrbitalSatOrbitCalculatorConfig,
    cache_dir: Arc<PathBuf>,
    orbit_calculation_task: Option<AbortHandle>,

}

impl LiveOrbitCalculator {
    pub fn new(config:  LiveOrbitalSatOrbitCalculatorConfig ) -> Self {
        let cache_dir= Arc::new( odin_build::cache_dir().join("orbital"));
        ensure_writable_dir(cache_dir.as_ref()).unwrap(); 
        LiveOrbitCalculator { 
            config: config,
            cache_dir: cache_dir,
            orbit_calculation_task: None
        }        
    }

    fn initialize(&mut self,  hself: ActorHandle<OrbitActorMsg>) -> Result<()> {
        println!("initializing orbit calc");
        self.orbit_calculation_task = Some( self.spawn_orbit_calculation_task( hself.clone(), self.cache_dir.clone() )? );
        Ok(())
    }

    fn spawn_orbit_calculation_task (&mut self, hself: ActorHandle<OrbitActorMsg>, cache_dir: Arc<PathBuf> ) -> Result<AbortHandle> {
        let config = self.config.clone();
        Ok( spawn( &format!("orbital-{}-orbit-calculation", self.config.satellite), async move {
            run_orbit_calculation( hself, config, cache_dir ).await
        })?.abort_handle()
        )
    }

    async fn calc_init_overpasses(&mut self, hself: ActorHandle<OrbitActorMsg>) -> Result<()> {
        println!("calc init overpasses");
        let tle = get_tles_celestrak(self.config.satellite).await?;
        let overpass = compute_initial_orbits(tle, self.config.max_scan_angle, chrono::Duration::from_std(self.config.history)?, &self.config.full_region)?;
        hself.try_send_msg(InitOverpassList(overpass))?;
        Ok(())
    }
}

 impl OrbitCalculator for LiveOrbitCalculator {
    fn calc_overpass_list (&self, overpass_request: &OverpassRequest, current_overpasses: &OverpassList ) -> Result<OverpassList> {
        let overpasses = get_overpasses_for_small_region(&overpass_request.region, current_overpasses, overpass_request.scan_angle);
        Ok(overpasses)
    }

    fn start(&mut self, hself: ActorHandle<OrbitActorMsg>) -> Result<()> {
        self.initialize(hself)?;
        Ok(())
    }

    async fn init(&mut self, hself: ActorHandle<OrbitActorMsg>) -> Result<()> {
        self.calc_init_overpasses(hself).await;
        Ok(())
    }
 }

 async fn run_orbit_calculation( hself: ActorHandle<OrbitActorMsg>, config: LiveOrbitalSatOrbitCalculatorConfig, cache_dir: Arc<PathBuf>) -> Result<()> {
    loop {
        sleep(config.calculation_interval).await;
        let tle = get_tles_celestrak(config.satellite).await?;
        let overpass = compute_full_orbits(tle, config.max_scan_angle, &config.full_region)?;
        hself.try_send_msg(UpdateOverpassList(overpass))?;
    }
    Ok(())
 }