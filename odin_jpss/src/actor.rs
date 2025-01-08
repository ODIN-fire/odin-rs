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
use std::io::Write;
use std::time::Duration;
use odin_common::geo::LatLon;
use odin_actor::prelude::*;
use odin_actor::{error,debug,warn,info};
use crate::orekit::{OrbitalTrajectory, OverpassList};
use crate::{RawHotspots, ViirsHotspotMap, ViirsHotspots};
use crate::errors::OdinJpssError;
use crate::errors::Result;
use crate::process_hotspots;

 // config
 #[derive(Serialize,Deserialize,Debug,Clone)]
pub struct JpssConfig {
    pub satellite: u32,
    pub source: String,
    pub max_age: Duration
}

 // external messages

 #[derive(Debug)]
 pub struct OverpassRequest {
    pub sat_id: u32, // 43013, 
    pub scan_angle: f64, // 56.2
    pub history: Duration, // 3 days  Duration(secs:604800, nanos:0) - pulls overpasses for past three days plus for next day
    pub region: Vec<LatLon>, // bounding box[ LatLon(lat_deg: 60.0, lon_deg: -135.0), LatLon(lat_deg: 60.0, lon_deg: -95.0), LatLon(lat_deg: 30.0, lon_deg: -95.0), LatLon(lat_deg: 30.0, lon_deg: -135.0), ],
    pub requester: DynMsgReceiver<UpdateOverpassList> // actor handle 
 }

 
 // exec snapshotaction
 #[derive(Debug)] pub struct ExecSnapshotAction(DynDataRefAction<ViirsHotspots>);


 /* #region JPSS import actor *************************************************************************************************/

 // internal message 
#[derive(Debug)] pub struct UpdateHotspots(pub(crate) ViirsHotspots);

#[derive(Debug)] pub struct UpdateRawHotspots(pub(crate) RawHotspots);

#[derive(Debug, Clone)] pub struct UpdateOverpassList(pub(crate) OverpassList);

#[derive(Debug)] pub struct ImportError(pub(crate) OdinJpssError);

define_actor_msg_set! { pub JpssImportActorMsg = OrbitsReady | ExecSnapshotAction | UpdateHotspots | UpdateOverpassList | ImportError | UpdateRawHotspots }

 // actor struct
 #[derive(Debug)]
pub struct JpssImportActor<T, HotspotUpdateAction, OverpassUpdateAction> 
    where T: JpssImporter + Send, 
        HotspotUpdateAction: DataAction<ViirsHotspots>,
        OverpassUpdateAction: DataAction<OverpassList>
{   source: String,
    satellite: u32,
    max_age: Duration,
    hotspots: ViirsHotspotMap,
    overpass_list: OverpassList,
    jpss_importer: T,
    hs_update_action: HotspotUpdateAction,
    op_update_action: OverpassUpdateAction,
    orbit_calculator: ActorHandle<OrbitActorMsg>
}
 // 3 ops, when last overpass is going to happen ask for new list, then ask for new use job scheduler, 

 // new, init, update
 // actor impl start, execsnapshot action, initialize, update, import error, terminate
impl <T, HotspotUpdateAction, OverpassUpdateAction> JpssImportActor <T, HotspotUpdateAction, OverpassUpdateAction> 
    where T: JpssImporter + Send, 
          HotspotUpdateAction: DataAction<ViirsHotspots>,
          OverpassUpdateAction: DataAction<OverpassList>
{
    pub fn new (config: JpssConfig, jpss_importer:T, hs_update_action: HotspotUpdateAction, op_update_action: OverpassUpdateAction, orbit_calculator:ActorHandle<OrbitActorMsg>) -> Self {
        let hotspots: ViirsHotspotMap = ViirsHotspotMap::new(config.satellite.clone(), config.source.clone());
        let overpass_list: OverpassList = OverpassList::new();
        JpssImportActor{source: config.source, max_age: config.max_age, satellite: config.satellite, hotspots, overpass_list, jpss_importer, hs_update_action, op_update_action, orbit_calculator}
    }


    pub async fn update_hotspots (&mut self, new_hotspots: ViirsHotspots) {
        self.hotspots.update(new_hotspots.clone(), self.max_age);
        let data_dir = PathBuf::from("C:\\Users\\srandrad\\odin\\odin_rs\\odin_jpss\\");
        let filename = data_dir.join(format!("hotspots_{}.json", self.source));
        let mut file = fs::File::create(filename).unwrap(); // don't use path yet as that would expose partial downloads to the world
        file.write(serde_json::to_string_pretty(&new_hotspots).unwrap().as_bytes());
        self.hs_update_action.execute(new_hotspots).await;
    }

    pub async fn update_overpass_list (&mut self, overpass_list: OverpassList) {
        self.overpass_list.update(overpass_list.clone());
        let data_dir = PathBuf::from("C:\\Users\\srandrad\\odin\\odin_rs\\odin_jpss\\");
        let ot = overpass_list.overpasses[0].clone();
        let filename = data_dir.join(format!("{}_{}.json", self.source, ot.t_start.format("%Y-%m-%d_H-%M-%S")));
        let mut file = fs::File::create(filename).unwrap(); // don't use path yet as that would expose partial downloads to the world
        file.write(serde_json::to_string_pretty(&ot).unwrap().as_bytes());
        self.op_update_action.execute(overpass_list).await;
    }

    pub fn process_raw_hotspots (&mut self, raw_hotspots:RawHotspots ) -> Result<ViirsHotspots> {
        let hotspots = process_hotspots( raw_hotspots, &self.overpass_list, self.satellite.clone(), self.source.clone())?;
        println!("hotspots: {:?}", hotspots);
        Ok(hotspots)
    }
}
 
impl_actor! { match msg for Actor< JpssImportActor<T, HotspotUpdateAction, OverpassUpdateAction>, JpssImportActorMsg> 
    where T:JpssImporter + Send + Sync, 
          HotspotUpdateAction: DataAction<ViirsHotspots> + Sync,
          OverpassUpdateAction: DataAction<OverpassList>
    as
    OrbitsReady => cont! { 
        println!("starting importer");
        let hself = self.hself.clone();
        let orbit_calculator =  self.orbit_calculator.clone() ;
        self.jpss_importer.start( hself, orbit_calculator ); //
    }

    ExecSnapshotAction => cont! { msg.0.execute( &self.hotspots.to_hotspots()).await; }

    UpdateRawHotspots => cont! { 
        println!("got raw hotspots");
        match self.process_raw_hotspots(msg.0) {
            Ok(hs) => { self.hself.try_send_msg(UpdateHotspots(hs)); },
            Err(e) => warn!("failed to process hotspots: {:?}", e)
        };
    }

    UpdateHotspots => cont! { self.update_hotspots(msg.0).await; }

    UpdateOverpassList => cont! { 
        self.update_overpass_list(msg.0).await; 
    }

    ImportError => cont! { error!("{:?}", msg.0); }
    
    _Terminate_ => stop! { self.jpss_importer.terminate(); }
}

 // abstraction trait
 pub trait JpssImporter {
    fn start (&mut self, hself: ActorHandle<JpssImportActorMsg>, orbit_handle: ActorHandle<OrbitActorMsg>) -> Result<()>;
    fn terminate (&mut self);
}

 /* #endregion JPSS import actor*/

 /* #region orbit calculator actor *************************************************************************************************/
 

 #[derive(Debug)]pub struct OrbitsReady;
#[derive(Debug)] pub struct AskOverpassRequest (pub(crate) OverpassRequest); 
#[derive(Debug)] pub struct InitOverpassList (pub(crate) OverpassList); 

 define_actor_msg_set! { pub OrbitActorMsg = InitOverpassList | AskOverpassRequest | Query<AskOverpassRequest, UpdateOverpassList> |  UpdateOverpassList}

pub struct OrbitActor <T, InitDataAction, UpdateAction> 
    where T: OrbitCalculator + Send, 
        InitDataAction: DataAction<OrbitsReady>,
        UpdateAction: DataAction<OverpassList>
{ 
    pub overpasses: OverpassList, // map of satellite ids and overpasses
    pub orbit_calculator: T,
    init_action: InitDataAction,
    update_action: UpdateAction
}

impl <T, InitDataAction, UpdateAction> OrbitActor <T, InitDataAction, UpdateAction> 
    where T: OrbitCalculator + Send,
        InitDataAction: DataAction<OrbitsReady>,
        UpdateAction: DataAction<OverpassList>
{
    pub fn new (orbit_calculator:T, init_action: InitDataAction, update_action:UpdateAction) -> Self {
        OrbitActor {orbit_calculator, overpasses: OverpassList::new(), init_action, update_action}
    }

    pub fn update_overpass_list(&mut self, new_overpasses: OverpassList) {
        self.overpasses.update(new_overpasses);
    }
}

impl_actor! { match msg for Actor< OrbitActor <T,InitDataAction, UpdateAction>, OrbitActorMsg> 
    where T: OrbitCalculator + Send, 
        InitDataAction: DataAction<OrbitsReady>,
        UpdateAction: DataAction<OverpassList>
    as
    _Start_ => cont! { 
        let hself = self.hself.clone();
        self.orbit_calculator.init( hself ).await; // calculates first overpass list and sends to self
    }

    InitOverpassList => cont! {
        println!("got init orbits");
        let data_dir = PathBuf::from("C:\\Users\\srandrad\\odin\\odin_rs\\odin_jpss\\");
        let ot = msg.0.overpasses[0].clone();
        let filename = data_dir.join(format!("init_{}_{}.json", String::from("NOAA20"), ot.t_start.format("%Y-%m-%d_H-%M-%S")));
        let mut file = fs::File::create(filename).unwrap(); // don't use path yet as that would expose partial downloads to the world
        file.write(serde_json::to_string_pretty(&ot).unwrap().as_bytes());
        let hself = self.hself.clone();
        self.update_overpass_list(msg.0); // updates overpass list
        self.orbit_calculator.start( hself ); // starts task that continuously calculates orbits
        println!("started orbit calculator");
        self.init_action.execute(OrbitsReady{}).await; // sends message to JpssImportActor to start
    }

    UpdateOverpassList => cont! {
        self.update_overpass_list(msg.0.clone());
        self.update_action.execute(msg.0);
    }

    AskOverpassRequest => cont! {
        println!("asking for overpasses");
        let overpass_list_res = self.orbit_calculator.calc_overpass_list(&msg.0, &self.overpasses);
        match overpass_list_res {
            Ok(overpass_list) => { msg.0.requester.send_msg(UpdateOverpassList(overpass_list)).await.unwrap(); }
            Err(e) => warn!("failed to calculate orbit: {}", e),
        }
    }

    Query<AskOverpassRequest, UpdateOverpassList> => cont! {
        println!("asking for overpasses");
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
    fn init(&mut self, hself: ActorHandle<OrbitActorMsg>) -> impl Future< Output = Result<()>> + Send; // completes first orbit calculation to kick off jpss importer and orbit calculation task
    fn start(&mut self, hself: ActorHandle<OrbitActorMsg>) -> Result<()>; // starts task to calculate orbits every so often
    fn calc_overpass_list (&self, overpass_request: &OverpassRequest, current_overpasses: &OverpassList ) -> Result<OverpassList>; // equivalent of JpssActor get_overpasses
 }

 
 /* #endregion orbit calculator actor*/

 /*orbit actor: 
 - takes in request message
 - gets overpasses
 - responds with overpasses
  */
