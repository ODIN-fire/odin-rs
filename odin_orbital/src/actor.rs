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
use std::collections::HashMap;
use std::fs;
use std::future::Future;
use std::iter::Map;
use std::path::PathBuf;
use std::time::Duration;
use odin_common::geo::{GeoCoord, GeoRect};
use odin_actor::prelude::*;
use odin_actor::{error,debug,warn,info};
use crate::overpass::OverpassList;
use crate::{RawHotspots, ViirsHotspot, ViirsHotspotSet, ViirsHotspotStore};
use crate::errors::OdinOrbitalSatError;
use crate::errors::Result;
use crate::process_hotspots;

 // config
 #[derive(Serialize,Deserialize,Debug,Clone)]
pub struct OrbitalSatConfig {
    pub satellite: u32,
    pub source: String,
    pub max_age: Duration
}

#[derive(Serialize,Deserialize,Debug,Clone)]
pub struct OrbitalSatImporterConfig {
   pub server: String,
   pub map_key: String,
   pub region: GeoRect,
   pub request_delay: Vec<Duration>,
}
#[derive(Serialize,Deserialize,Debug,Clone)]
pub struct OrbitalSatOrbitCalculatorConfig {
   pub full_region: GeoRect,
   pub calculation_interval: Duration,
}

 // external messages

 #[derive(Debug)]
 pub struct OverpassRequest {
    pub sat_id: u32, // 43013, 
    pub scan_angle: f64, // 56.2
    pub history: Duration, // 3 days  Duration(secs:604800, nanos:0) - pulls overpasses for past three days plus for next day
    pub region: GeoRect, // bounding box[ GeoCoord(lat_deg: 60.0, lon_deg: -135.0), GeoCoord(lat_deg: 60.0, lon_deg: -95.0), GeoCoord(lat_deg: 30.0, lon_deg: -95.0), GeoCoord(lat_deg: 30.0, lon_deg: -135.0), ],
    pub requester: DynMsgReceiver<UpdateOverpassList> // actor handle 
 }

 
 // exec snapshotaction
 #[derive(Debug)] pub struct ExecSnapshotAction( pub DynDataRefAction<ViirsHotspotStore>);
 #[derive(Debug)] pub struct ExecOverpassSnapshotAction( pub DynDataRefAction<OverpassList>);


 /* #region OrbitalSat import actor *************************************************************************************************/

 // internal message 
 
#[derive(Debug)] pub struct InitialHotspots(pub(crate) RawHotspots);
#[derive(Debug)] pub struct UpdateHotspots(pub(crate) ViirsHotspotSet);

#[derive(Debug)] pub struct UpdateRawHotspots(pub(crate) RawHotspots);

#[derive(Debug, Clone)] pub struct UpdateOverpassList(pub(crate) OverpassList);

#[derive(Debug)] pub struct ImportError(pub(crate) OdinOrbitalSatError);

define_actor_msg_set! { pub OrbitalSatImportActorMsg = ExecSnapshotAction | ExecOverpassSnapshotAction | OrbitsReady | InitialHotspots | UpdateHotspots | UpdateOverpassList | ImportError | UpdateRawHotspots }
// TODO: add init of sending the store
 // actor struct
 #[derive(Debug)] 
pub struct OrbitalSatImportActor<T, InitAction, HotspotUpdateAction, OverpassUpdateAction> 
    where T: OrbitalSatImporter + Send, 
        InitAction: DataRefAction<ViirsHotspotStore>,
        HotspotUpdateAction: DataAction<ViirsHotspotSet>,
        OverpassUpdateAction: DataAction<OverpassList>
{   source: String,
    satellite: u32,
    max_age: Duration,
    hotspots: ViirsHotspotStore,
    overpass_list: OverpassList,
    orbital_importer: T,
    init_action: InitAction,
    hs_update_action: HotspotUpdateAction,
    op_update_action: OverpassUpdateAction,
    orbit_calculator: ActorHandle<OrbitActorMsg>
}
 // 3 ops, when last overpass is going to happen ask for new list, then ask for new use job scheduler, 

 // new, init, update
 // actor impl start, execsnapshot action, initialize, update, import error, terminate
impl <T, InitAction, HotspotUpdateAction, OverpassUpdateAction> OrbitalSatImportActor <T, InitAction, HotspotUpdateAction, OverpassUpdateAction> 
    where T: OrbitalSatImporter + Send, 
          InitAction: DataRefAction<ViirsHotspotStore>,
          HotspotUpdateAction: DataAction<ViirsHotspotSet>,
          OverpassUpdateAction: DataAction<OverpassList>
{
    pub fn new (config: OrbitalSatConfig, orbital_importer:T, init_action: InitAction, hs_update_action: HotspotUpdateAction, op_update_action: OverpassUpdateAction, orbit_calculator:ActorHandle<OrbitActorMsg>) -> Self {
        let hotspots: ViirsHotspotStore = ViirsHotspotStore::new(config.satellite.clone(), config.source.clone());
        let overpass_list: OverpassList = OverpassList::new();
        OrbitalSatImportActor{source: config.source, max_age: config.max_age, satellite: config.satellite, hotspots, overpass_list, orbital_importer, init_action, hs_update_action, op_update_action, orbit_calculator}
    }

    pub async fn process_initial_hotspots(&mut self, init_hotspots: RawHotspots) -> Result<()> {
        let hotspots = process_hotspots( init_hotspots, &self.overpass_list, self.satellite.clone(), self.source.clone())?;
        for hs in hotspots.into_iter() {
          self.hotspots.update(hs, self.max_age);
        }
        self.init_action.execute(&self.hotspots).await;
        Ok(())
    }

    pub async fn update_hotspots (&mut self, new_hotspots: ViirsHotspotSet) -> Result<()> {
        self.hotspots.update(new_hotspots.clone(), self.max_age);
        self.hs_update_action.execute(new_hotspots).await;
        Ok(())
    }

    pub async fn update_overpass_list (&mut self, overpass_list: OverpassList) -> Result<()> {
        self.overpass_list.update(overpass_list.clone());
        self.op_update_action.execute(overpass_list).await;
        Ok(())
    }

    pub fn process_raw_hotspots (&mut self, raw_hotspots:RawHotspots ) -> Result<Vec<ViirsHotspotSet>> {
        let hotspots = process_hotspots( raw_hotspots, &self.overpass_list, self.satellite.clone(), self.source.clone())?;
        Ok(hotspots)
    }
}
 
impl_actor! { match msg for Actor< OrbitalSatImportActor<T, InitAction, HotspotUpdateAction, OverpassUpdateAction>, OrbitalSatImportActorMsg> 
    where T:OrbitalSatImporter + Send + Sync, 
          InitAction: DataRefAction<ViirsHotspotStore>,
          HotspotUpdateAction: DataAction<ViirsHotspotSet> + Sync,
          OverpassUpdateAction: DataAction<OverpassList>
    as
    

    ExecSnapshotAction => cont! { msg.0.execute( &self.hotspots ).await; }

    ExecOverpassSnapshotAction => cont! { msg.0.execute( &self.overpass_list ).await; }

    OrbitsReady => cont! { 
        let hself = self.hself.clone();
        let orbit_calculator =  self.orbit_calculator.clone() ;
        self.orbital_importer.start( hself, orbit_calculator ); 
    }

    InitialHotspots => cont! { self.process_initial_hotspots(msg.0).await; }

    UpdateRawHotspots => cont! { 
        match self.process_raw_hotspots(msg.0) {
            Ok(hs) => { hs.into_iter().map(|hs_set| self.hself.try_send_msg(UpdateHotspots(hs_set))); },
            Err(e) => warn!("failed to process hotspots: {:?}", e)
        };
    }

    UpdateHotspots => cont! { self.update_hotspots(msg.0).await; }

    UpdateOverpassList => cont! { self.update_overpass_list(msg.0).await; }

    ImportError => cont! { error!("{:?}", msg.0); }
    
    _Terminate_ => stop! { self.orbital_importer.terminate(); }
}

 // abstraction trait
 pub trait OrbitalSatImporter {
    fn start (&mut self, hself: ActorHandle<OrbitalSatImportActorMsg>, orbit_handle: ActorHandle<OrbitActorMsg>) -> Result<()>;
    fn terminate (&mut self);
}

 /* #endregion OrbitalSat import actor*/

 /* #region orbit calculator actor *************************************************************************************************/
 

 #[derive(Debug)]pub struct OrbitsReady;
#[derive(Debug)] pub struct AskOverpassRequest (pub(crate) OverpassRequest); 
#[derive(Debug)] pub struct InitOverpassList (pub(crate) OverpassList); 

 define_actor_msg_set! { pub OrbitActorMsg = InitOverpassList | AskOverpassRequest | Query<AskOverpassRequest, UpdateOverpassList> |  UpdateOverpassList}

pub struct OrbitActor <T, InitDataAction> 
    where T: OrbitCalculator + Send, 
        InitDataAction: DataAction<OrbitsReady>,
        //UpdateAction: DataAction<OverpassList>
{ 
    pub overpasses: OverpassList, // map of satellite ids and overpasses
    pub orbit_calculator: T,
    init_action: InitDataAction,
    //update_action: UpdateAction
}

impl <T, InitDataAction> OrbitActor <T, InitDataAction> 
    where T: OrbitCalculator + Send,
        InitDataAction: DataAction<OrbitsReady>,
        //UpdateAction: DataAction<OverpassList>
{
    pub fn new (orbit_calculator:T, init_action: InitDataAction) -> Self {
        OrbitActor {orbit_calculator, overpasses: OverpassList::new(), init_action}
    }

    pub fn update_overpass_list(&mut self, new_overpasses: OverpassList) {
        self.overpasses.update(new_overpasses);
    }
}

impl_actor! { match msg for Actor< OrbitActor <T,InitDataAction>, OrbitActorMsg> 
    where T: OrbitCalculator + Send, 
        InitDataAction: DataAction<OrbitsReady>
        //UpdateAction: DataAction<OverpassList>
    as
    _Start_ => cont! { 
        let hself = self.hself.clone();
        self.orbit_calculator.init( hself ).await; // calculates first overpass list and sends to self
    }

    InitOverpassList => cont! {
        let hself = self.hself.clone();
        self.update_overpass_list(msg.0); // updates overpass list
        self.orbit_calculator.start( hself ); // starts task that continuously calculates orbits
        self.init_action.execute(OrbitsReady{}).await; // sends message to OrbitalSatImportActor to start
    }

    UpdateOverpassList => cont! {
        self.update_overpass_list(msg.0.clone());
        // self.update_action.execute(msg.0);
    }

    AskOverpassRequest => cont! {
        let overpass_list_res = self.orbit_calculator.calc_overpass_list(&msg.0, &self.overpasses);
        match overpass_list_res {
            Ok(overpass_list) => { msg.0.requester.send_msg(UpdateOverpassList(overpass_list)).await.unwrap(); }
            Err(e) => warn!("failed to calculate orbit: {}", e),
        }
    }

    Query<AskOverpassRequest, UpdateOverpassList> => cont! {
        let overpass_list_res = self.orbit_calculator.calc_overpass_list(&msg.question.0, &self.overpasses);
        match overpass_list_res {
            Ok(overpass_list) => match msg.respond(UpdateOverpassList(overpass_list)).await {
                Ok(()) => { info!("sent an overpass list") },
                Err(e) => warn!("failed to send overpasses: {}", e)
            }
            Err(e) => warn!("failed to calculate orbit: {}", e)
        }
        
    }
}

 pub trait OrbitCalculator {
    fn init(&mut self, hself: ActorHandle<OrbitActorMsg>) -> impl Future< Output = Result<()>> + Send; // completes first orbit calculation to kick off OrbitalSat importer and orbit calculation task
    fn start(&mut self, hself: ActorHandle<OrbitActorMsg>) -> Result<()>; // starts task to calculate orbits every so often
    fn calc_overpass_list (&self, overpass_request: &OverpassRequest, current_overpasses: &OverpassList ) -> Result<OverpassList>; // equivalent of OrbitalSatActor get_overpasses
 }

 
 /* #endregion orbit calculator actor*/

 /*orbit actor: 
 - takes in request message
 - gets overpasses
 - responds with overpasses
  */
