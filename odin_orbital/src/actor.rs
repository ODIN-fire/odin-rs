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

use std::{collections::VecDeque, sync::Arc, time::Duration, path::{Path,PathBuf}};

use chrono::{DateTime,Utc,Local};
use odin_build::pkg_cache_dir;
use odin_actor::prelude::*;
use odin_job::JobScheduler;
use odin_common::{collections::{empty_vec, RingDeque}, datetime, fs::set_filepath_contents, geo::GeoPolygon};
use odin_macro::public_struct;
use satkit::consts::C;
use uom::si::volume_rate::gallon_imperial_per_second;
use crate::{
    duration_days, duration_minutes, save_retrieved_hotspots_to, update_orbital_data, 
    errors::{action_failed, OdinOrbitalError, Result}, init_orbital_data, instant_from_datetime, instant_now, 
    overpass::{self, OverpassCalculator, save_overpasses_to}, 
    tle_store::SpaceTrackTleStore, 
    CompletedOverpass, HotspotImporter, HotspotList, OrbitalSatelliteInfo, Overpass, TleStore
}; 

macro_rules! info { ($fmt:literal $(, $arg:expr )* ) => { {print!("INFO: "); println!( $fmt $(, $arg)* )} } }
macro_rules! error { ($fmt:literal $(, $arg:expr )* ) => { {eprint!("\x1b[32;1m \x1b[37m ERROR: "); eprint!( $fmt $(, $arg)* ); eprintln!("\x1b[0m")} } }


pub struct HotspotActorData {
    pub completed: VecDeque<CompletedOverpass<HotspotList>>,
    pub upcoming: VecDeque<Overpass>,
}

/// actor producing overpasses and hotspot data for a single satellite  
pub struct OrbitalHotspotActor <T,I,A,O,H> 
    where T: TleStore + Send, I: HotspotImporter + Send, A: DataRefAction<HotspotActorData>, O: DataRefAction<Overpass>, H: DataRefAction<HotspotList>
{
    sat_info: Arc<OrbitalSatelliteInfo>,

    importer: I,
    overpass_calculator: OverpassCalculator<T>,
    scheduler: JobScheduler,

    data: HotspotActorData,
    cache_dir: PathBuf,

    //--- our connectors
    init_action: A, // to announce once we have data
    overpass_action: O, // to be executed when there is a new overpass
    hotspot_action: H, // to be executed when we have new data for an overpass
}

impl <T,I,A,O,H> OrbitalHotspotActor <T,I,A,O,H> 
    where T: TleStore + Send, I: HotspotImporter + Send, A: DataRefAction<HotspotActorData>, O: DataRefAction<Overpass>, H: DataRefAction<HotspotList>
{
    pub fn new (sat_info: Arc<OrbitalSatelliteInfo>, region: GeoPolygon, tle_store: T, importer: I, init_action:A, overpass_action:O, hotspot_action:H)->Self {
        let overpass_calculator = OverpassCalculator::new(sat_info.clone(), region, tle_store);
        let max_completed = sat_info.max_completed;
        let max_upcoming = sat_info.max_upcoming;

        let data = HotspotActorData {
            completed: VecDeque::with_capacity(max_completed),
            upcoming: VecDeque::with_capacity(max_upcoming),
        };
        let cache_dir = pkg_cache_dir!();
        let scheduler = JobScheduler::new();

        Self { sat_info, importer, overpass_calculator, scheduler, data, cache_dir, init_action, overpass_action, hotspot_action }
    }

    async fn start (&mut self, hself: ActorHandle<OrbitalHotspotActorMsg>)->Result<()> {
        self.scheduler.run()?;

        self.overpass_calculator.initialize().await?;
        let mut overpasses = self.overpass_calculator.get_initial_overpasses().await?;
        info!("start got {} initial overpasses", overpasses.len());
        save_overpasses_to( &self.cache_dir, &overpasses)?;

        // obtain and sort in overpasses for the region
        let now = datetime::utc_now();
        while let Some(o) = overpasses.pop() { // note this is in reverse order so we have to push to front
            if o.end < now { // completed - note that we partition on end to make sure we don't miss data for an ongoing overpass
                self.data.completed.push_front( CompletedOverpass::new(o));
            } else {
                self.data.upcoming.push_front( o);
            }
        }

        // if we have completed overpasses retrieve hotspot data for it
        if !self.data.completed.is_empty() { 
            let retrieved = self.importer.import_hotspots( self.sat_info.back_days, &mut self.data.completed).await?;
            info!("start got {} initial hotspots", retrieved.len());
            save_retrieved_hotspots_to( &self.cache_dir, &retrieved, &self.data.completed)?;
        }

        // if we have some data exec our init slot
        if !self.data.completed.is_empty() || !self.data.upcoming.is_empty() {
            self.init_action.execute( &self.data).await?;
        }

        self.schedule_next_retrieval( hself);

        Ok(())
    }

    fn schedule_next_retrieval (&mut self, hself: ActorHandle<OrbitalHotspotActorMsg>) {
        let import_schedule = self.get_next_schedule();

        for t in &import_schedule {
            self.scheduler.schedule_at( t, {
                info!("scheduled next retrieval at {} (local {})", t, DateTime::<Local>::from(*t));
                let hself = hself.clone();
                move |_ctx| {
                    hself.retry_send_msg( 3, Duration::from_secs(10), RetrieveData{}); // retry if queue is full
                } 
            });
        }
    }

    fn get_next_schedule (&self)->Vec<DateTime<Utc>> {
        if !self.data.upcoming.is_empty() {
            self.importer.get_download_schedule( self.data.upcoming.front().unwrap().end)
        } else {
            vec![ datetime::utc_now() + Duration::from_hours(1) ] // no upcomings - check again in an hour
        }
    }

    async fn snapshot (&mut self)->Result<()> {
        for c in &self.data.completed {
            self.overpass_action.execute( &c.overpass).await?;
            if let Some(hs) = &c.data {
                self.hotspot_action.execute( hs).await?;
            }
        }
        for o in &self.data.upcoming {
            self.overpass_action.execute( o).await?;
        }

        Ok(())
    }

    async fn retrieve_data (&mut self, hself: ActorHandle<OrbitalHotspotActorMsg>)->Result<()> {
        let mut n_completed = 0;

        // move all upcoming overpasses that have passed into completed
        // note there might not be any (either because we had no upcoming overpasses yet or this is an update for a previously completed one)
        while let Some(o) = self.data.upcoming.front() {
            if o.end < datetime::utc_now() {
                let co = CompletedOverpass::new( self.data.upcoming.pop_front().unwrap());
                self.data.completed.push_to_ringbuffer( co);
                n_completed += 1;
            } else { break }
        }
        info!("retrieve moved {} overpasses from upcoming to completed", n_completed);

        // now retrieve current hotspot data (might span several completed overpasses) - if we have completeds without data

        if let Some(last_completed) = self.data.completed.back() {
            if last_completed.data.is_none() { // we could also check if it had URT entries
                let retrieved = self.importer.import_hotspots( 1, &mut self.data.completed).await?;
                info!("retrieved {} hotspots", retrieved.len());
                save_retrieved_hotspots_to( &self.cache_dir, &retrieved, &self.data.completed)?;

                // execute hotspot action for each of the retrieved Hotspot lists
                for idx in retrieved.iter() {
                    let co = &self.data.completed[idx];

                    if let Some(hs) = &co.data {
                        self.hotspot_action.execute( hs).await?;
                    }
                }
            }
        }

        // we moved at least one upcoming to completed - get new upcoming overpasses
        if n_completed > 0 || self.data.upcoming.is_empty() {
            let t = if let Some(o) = self.data.upcoming.back() { instant_from_datetime(o.end) + duration_minutes(20) } else { instant_now() };
            let mut overpasses = self.overpass_calculator.get_overpasses( t, duration_days(1)).await?;
            info!("retrieved {} new overpasses", overpasses.len());
            save_overpasses_to( &self.cache_dir, &overpasses)?;

            overpasses.reverse();
            while let Some(o) = overpasses.pop() {
                if self.data.upcoming.len() < self.sat_info.max_upcoming {
                    self.overpass_action.execute( &o).await?;
                    self.data.upcoming.push_back(o);
                }
            }
        }

        // and finally schedule our next invocation
        self.schedule_next_retrieval(hself); 

        Ok(())
    }

    async fn terminate (&mut self)->Result<()> {
        self.scheduler.abort();
        Ok(())
    }

}


/// internal message that triggers retrieval of new hotspots for completed overpasses and new future overpasses
#[derive(Debug,Clone)] pub struct RetrieveData{}

/// external message to request current overpasses and hotspots
#[derive(Debug)] pub struct ExecSnapshotAction(pub DynDataRefAction<HotspotActorData>);

define_actor_msg_set! { pub OrbitalHotspotActorMsg = RetrieveData  | ExecSnapshotAction }

impl_actor! { match msg for Actor<OrbitalHotspotActor<T,I,A,O,H>, OrbitalHotspotActorMsg> 
    where T: TleStore + Send, I: HotspotImporter + Send, A: DataRefAction<HotspotActorData>, O: DataRefAction<Overpass>, H: DataRefAction<HotspotList>
as
    _Start_ => cont! { 
        let hself = self.hself.clone();
        if let Err(e) = self.start( hself).await { error!("start failed {e}") } 
    }

    ExecSnapshotAction => cont! {
       if let Err(e) = self.snapshot().await { error!("snapshot failed {e}") }
    }

    RetrieveData => cont! {
        let hself = self.hself.clone();
        if let Err(e) = self.retrieve_data(hself).await { error!("retrieve data failed {e}") }
    }
    
    _Terminate_ => stop! { 
        self.terminate().await;
    }
}
