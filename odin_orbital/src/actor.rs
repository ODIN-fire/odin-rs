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
use chrono::{DateTime, Local, TimeDelta, Utc};
use satkit::consts::C;
use uom::si::volume_rate::gallon_imperial_per_second;

use odin_build::pkg_cache_dir;
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_job::JobScheduler;
use odin_common::{
    collections::{empty_vec, RefVec, RingDeque},
    datetime, fs::{remove_old_files, set_filepath_contents}, geo::GeoPolygon, 
    json_writer::{JsonWritable, JsonWriter}
};
use odin_macro::public_struct;
use crate::{
    duration_days, duration_minutes, errors::{action_failed, OdinOrbitalError, Result}, firms::{ViirsHotspotImporter,OliHotspotImporter}, hotspot_service::{HotspotSat,OrbitalHotspotService}, init_orbital_data, instant_from_datetime, instant_now, load_config, overpass::{self, save_overpasses_to, OverpassCalculator}, save_retrieved_hotspots_to, tle_store::SpaceTrackTleStore, update_orbital_data, CompletedOverpass, HotspotImporter, HotspotList, OrbitalSatelliteInfo, Overpass, TleStore
}; 

macro_rules! info { ($fmt:literal $(, $arg:expr )* ) => { {print!("INFO: "); println!( $fmt $(, $arg)* )} } }
macro_rules! error { ($fmt:literal $(, $arg:expr )* ) => { {eprint!("\x1b[32;1m \x1b[37m ERROR: "); eprint!( $fmt $(, $arg)* ); eprintln!("\x1b[0m")} } }


pub struct HotspotActorData {
    pub completed: VecDeque<CompletedOverpass<HotspotList>>,
    pub upcoming: VecDeque<Overpass>,
}

impl HotspotActorData {
    pub fn serialize_collapsed_overpasses (&self)->String {
        let mut w = JsonWriter::with_capacity( self.completed.len() + self.upcoming.len() * 128);
        w.write_array(|w|{
            for co in &self.completed { co.overpass.write_collapsed_json_to(w) }
            for o in &self.upcoming { o.write_collapsed_json_to(w) }
        });
        w.to_string()
    }

    pub fn serialize_collapsed_hotspots (&self)->String {
        let mut w = JsonWriter::with_capacity( self.completed.len() * 128);
        w.write_array(|w|{
            for co in &self.completed { 
                if let Some(hotspots) = &co.data {
                    hotspots.write_collapsed_json_to(w);
                }
            }
        });
        w.to_string()
    }
}

/// actor producing overpasses and hotspot data for a single satellite  
pub struct OrbitalHotspotActor <T,I,A,O,H> 
    where   
        T: TleStore + Send, 
        I: HotspotImporter + Send, 
        A: DataRefAction<HotspotActorData>, 
        O: for <'a> DataAction<Vec<&'a Overpass>>, 
        H: for <'a> DataAction<Vec<&'a HotspotList>>
{
    sat_info: Arc<OrbitalSatelliteInfo>,

    importer: I,
    overpass_calculator: OverpassCalculator<T>,
    scheduler: JobScheduler,

    data: HotspotActorData,
    cache_dir: PathBuf,

    //--- our connectors
    init_action: A, // to announce once we have data
    overpass_action: O, // to be executed when there are new overpasses
    hotspot_action: H, // to be executed when we have new hotspots for completed overpasses
}

impl <T,I,A,O,H> OrbitalHotspotActor <T,I,A,O,H> 
    where   
        T: TleStore + Send, 
        I: HotspotImporter + Send, 
        A: DataRefAction<HotspotActorData>, 
        O: for <'a> DataAction<Vec<&'a Overpass>>, 
        H: for <'a> DataAction<Vec<&'a HotspotList>>
{
    pub fn new (sat_info: Arc<OrbitalSatelliteInfo>, region: Arc<GeoPolygon>, tle_store: T, importer: I, init_action:A, overpass_action:O, hotspot_action:H)->Self {
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

            // remove trailing completed overpasses without hotspots
            while let Some(back) = self.data.completed.back() {
                if back.data.is_none() { 
                    info!("remove trailing completed without data");
                    self.data.completed.pop_back(); 
                } else { break; }
            }
        }

        // if we have some data exec our init slot
        if !self.data.completed.is_empty() || !self.data.upcoming.is_empty() {
            self.init_action.execute( &self.data).await?;
        }

        self.schedule_next_retrieval( hself);

        Ok(())
    }

    fn schedule_next_retrieval (&mut self, hself: ActorHandle<OrbitalHotspotActorMsg>) {
        let t = self.get_next_schedule();

        self.scheduler.schedule_at( &t, {
            info!("scheduled next retrieval at local {}", DateTime::<Local>::from(t));
            let hself = hself.clone();
            move |_ctx| {
                hself.retry_send_msg( 3, Duration::from_secs(10), RetrieveData{}); // retry if queue is full
            } 
        });
    }

    fn last_completed_has_data (&self)->bool {
        self.data.completed.back().and_then(|co| co.data.as_ref()).is_some()
    }

    fn get_next_schedule (&self)->DateTime<Utc> {
        let now = datetime::utc_now();

        if let Some(retry_date) = self.check_retry_schedule(&now) {
            retry_date

        } else { // this should be the normal branch - schedule (or obtain) the next overpass
            if let Some(next_upcoming) = self.data.upcoming.front() {
                if now < next_upcoming.end { // nominal case
                    self.importer.get_download_schedule( next_upcoming.end)
    
                } else { // TODO - next upcoming wasn't moved? this seems like an error
                    warn!("expired upcoming overpass {} at {}", next_upcoming.end, now);
                    now + Duration::from_mins(5)
                }
            } else { // no upcoming overpasses in time window - check again in an hour
                now + Duration::from_hours(1)
            }
        }
    }

    fn check_retry_schedule (&self, now: &DateTime<Utc>)-> Option<DateTime<Utc>> {
        if let Some(last) = self.data.completed.back() {
            if last.data.is_none() {  // we don't have data for the last completed yet
                if self.importer.last_reported() < last.overpass.end { // and we didn't get anything newer since then
                    if let Some(next) = self.data.upcoming.front() {
                        if (next.end - now) < TimeDelta::minutes(30) { // only retry download if the next overpass isn't close yet
                            return None
                        }
                    } else {
                        if (*now - last.overpass.end) > TimeDelta::minutes(30) {
                            return None
                        } 
                    }
                    return Some(*now + Duration::from_mins(10))
                } // otherwise it means we already got newer hotspots but there were none for this overpass
            } // otherwise we already have data for the last overpass
        } // there was no last overpass yet
        None
    }

    async fn exec_snapshot (&mut self, action: DynDataRefAction<HotspotActorData>)->Result<()> {
        action.execute(&self.data).await?;
        Ok(())
    }

    async fn retrieve_data (&mut self, hself: ActorHandle<OrbitalHotspotActorMsg>)->Result<()> {
        let mut n_completed = 0;

        self.drop_old_files(); // some house keeping first

        // move all upcoming overpasses that have passed into completed
        // note there might not be any (either because we had no upcoming overpasses yet or this is an update for a previously completed one)
        while let Some(o) = self.data.upcoming.front() {
            if o.end < datetime::utc_now() {
                let co = CompletedOverpass::new( self.data.upcoming.pop_front().unwrap());
                self.data.completed.push_to_ringbuffer( co); // this makes sure we drop old overpass data
                n_completed += 1;
            } else { break }
        }
        info!("retrieve moved {} overpasses from upcoming to completed", n_completed);

        // now retrieve current hotspot data (might span several completed overpasses) - if the last completed overpass doesn't have data yet
        if let Some(last_completed) = self.data.completed.back() {
            if last_completed.data.is_none() { // TODO - we could also check if it had URT entries
                let retrieved = self.importer.import_hotspots( 1, &mut self.data.completed).await?;
                info!("retrieved {} hotspots", retrieved.len());
                save_retrieved_hotspots_to( &self.cache_dir, &retrieved, &self.data.completed)?;

                let hotspots: Vec<&HotspotList> = retrieved.iter().filter_map( |i| self.data.completed[i].data.as_ref()).collect();
                if !hotspots.is_empty() {
                    self.hotspot_action.execute( hotspots).await?;
                }
            }
        }

        // we moved upcoming to completed - get up to n_completed new upcoming overpasses (we don't have yet)
        if n_completed > 0 || self.data.upcoming.is_empty() {
            let t = if let Some(o) = self.data.upcoming.back() { instant_from_datetime(o.end) + duration_minutes(20) } else { instant_now() };
            let mut overpasses = self.overpass_calculator.get_overpasses( t, duration_days( self.sat_info.forward_days), n_completed).await?;
            info!("retrieved {} new overpasses", overpasses.len());
            save_overpasses_to( &self.cache_dir, &overpasses)?;

            if !overpasses.is_empty() {
                self.overpass_action.execute( overpasses.as_ref_vec()).await?;
                for o in overpasses {
                    self.data.upcoming.push_back(o);
                }
            }
        }

        // and finally schedule our next invocation
        // watch out - don't immediately reschedule if we didn't get data for the last completed overpass (needs a delay) 
        self.schedule_next_retrieval(hself); 

        Ok(())
    }

    async fn terminate (&mut self)->Result<()> {
        self.scheduler.abort();
        Ok(())
    }

    fn drop_old_files (&self)->Result<()> {
        remove_old_files( &self.cache_dir, Duration::from_days(self.sat_info.back_days as u64 + 1))?;
        Ok(())
    }

}


/// internal message that triggers retrieval of new hotspots for completed overpasses and new future overpasses
#[derive(Debug,Clone)] pub struct RetrieveData{}

/// external message to request current overpasses and hotspots
#[derive(Debug)] pub struct ExecSnapshotAction(pub DynDataRefAction<HotspotActorData>);

define_actor_msg_set! { pub OrbitalHotspotActorMsg = RetrieveData  | ExecSnapshotAction }

impl_actor! { match msg for Actor<OrbitalHotspotActor<T,I,A,O,H>, OrbitalHotspotActorMsg> 
    where 
        T: TleStore + Send, 
        I: HotspotImporter + Send, 
        A: DataRefAction<HotspotActorData>, 
        O: for <'a> DataAction<Vec<&'a Overpass>>, 
        H: for <'a> DataAction<Vec<&'a HotspotList>>
    as

    _Start_ => cont! { 
        let hself = self.hself.clone();
        if let Err(e) = self.start( hself).await { error!("start failed {e}") } 
    }

    ExecSnapshotAction => cont! {
       if let Err(e) = self.exec_snapshot( msg.0).await { error!("snapshot failed {e}") }
    }

    RetrieveData => cont! {
        let hself = self.hself.clone();
        if let Err(e) = self.retrieve_data( hself).await { error!("retrieve data failed {e}") }
    }
    
    _Terminate_ => stop! { 
        self.terminate().await;
    }
}


pub fn spawn_orbital_hotspot_actors (actor_system: &mut ActorSystem, hserver: ActorHandle<SpaServerMsg>, 
                                                             region: GeoPolygon, sat_infos: &Vec<&str>) -> Result<Vec<HotspotSat>> 
{
    let cache_dir = pkg_cache_dir!();
    let region = Arc::new(region);
    let mut sats: Vec<HotspotSat> = Vec::with_capacity( sat_infos.len());

    for sat_info in OrbitalSatelliteInfo::from_filenames(sat_infos)? {
        let name = sat_info.name.clone();
        let sender_id = Arc::new( name.clone());

        // unfortunately we can't share actions between actors
        let init_action = dataref_action!( // init action - just let the service know we have data
            let hserver: ActorHandle<SpaServerMsg> = hserver.clone(),
            let sender_id: Arc<String> = sender_id.clone() => 
            |data: &HotspotActorData| { 
                Ok( hserver.retry_send_msg( 3, Duration::from_secs(10), DataAvailable::new::<HotspotActorData>(sender_id) )? )
            }
        );

        let overpass_action = data_action!( // update overpasses 
            let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => 
            |overpasses: Vec<&Overpass>| { // overpass action
                let ws_msg = ws_msg_from_json( OrbitalHotspotService::mod_path(), "overpasses", &Overpass::to_collapsed_json_array(&overpasses));
                Ok( hserver.retry_send_msg( 3, Duration::from_secs(10), BroadcastWsMsg { ws_msg })? )
            }
        );

        let hotspot_action = data_action!( // update hotspots
            let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => 
            |hotspots: Vec<&HotspotList>| { // hotspot action
                let ws_msg = ws_msg_from_json( OrbitalHotspotService::mod_path(), "hotspots", &HotspotList::to_collapsed_json_array(&hotspots));
                Ok( hserver.retry_send_msg( 3, Duration::from_secs(10), BroadcastWsMsg { ws_msg })? )
            }
        );

        let tle_store = SpaceTrackTleStore::new( load_config("spacetrack.ron")?, sat_info.clone(), Some(cache_dir.clone()));

        // TODO - this is suboptimal as it is a structural bottleneck. Maybe we should turn the importer into a Box<dyn HotspotImporter> 
        let hupdater = match sat_info.instrument.as_str() {
            "VIIRS" => {
                let importer = ViirsHotspotImporter::new( load_config("firms.ron")?, sat_info.clone(), cache_dir.clone());
                spawn_actor!( actor_system, name, 
                    OrbitalHotspotActor::new(sat_info.clone(), region.clone(), tle_store, importer, init_action, overpass_action, hotspot_action)
                )?
            }
            "OLI" => {
                let importer = OliHotspotImporter::new( load_config("firms.ron")?, sat_info.clone(), cache_dir.clone());
                spawn_actor!( actor_system, name, 
                    OrbitalHotspotActor::new(sat_info.clone(), region.clone(), tle_store, importer, init_action, overpass_action, hotspot_action)
                )?
            }
            unknown => panic!("no importer for instrument {unknown}") // Ok to panic since this is toplevel func
        };

        sats.push( HotspotSat { sat_info, hupdater });
    }

    Ok(sats)
}