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
use odin_common::geo::LatLon;
use odin_actor::prelude::*;
use core::future::Future;
use crate::orekit::{OrbitalTrajectory, OverpassList};
use crate::{ViirsHotspots, RawHotspots};
use crate::errors::OdinJpssError;
use crate::errors::Result;
use crate::process_hotspots;

 // config
 #[derive(Serialize,Deserialize,Debug,Clone)]
pub struct JpssConfig {
    pub satellite: u32,
    pub source: String
}

pub struct OrbitConfig {

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
 // update - new viirshotspots
 // update - new overpasses
 // initialize - viirshotspots - same behavior as update, just gives a new list
 // import error - error downloading 
#[derive(Debug)] pub struct UpdateHotspots(pub(crate) ViirsHotspots);

#[derive(Debug)] pub struct UpdateRawHotspots(pub(crate) RawHotspots);

#[derive(Debug)] pub struct UpdateOverpassList(pub(crate) OverpassList);
 // #[derive(Debug)] pub struct Initialize(pub(crate) ViirsHotspots); // same behavior as update?
#[derive(Debug)] pub struct ImportError(pub(crate) OdinJpssError);

define_actor_msg_set! { pub JpssImportActorMsg = ExecSnapshotAction | UpdateHotspots | UpdateOverpassList | ImportError | UpdateRawHotspots }

 // user part

 // actor struct
 #[derive(Debug)]
pub struct JpssImportActor<T, HotspotUpdateAction, OverpassUpdateAction> 
    where T: JpssImporter + Send, 
        HotspotUpdateAction: DataAction<ViirsHotspots>,
        OverpassUpdateAction: DataAction<OverpassList>
{   source: String,
    satellite: u32,
    hotspots: ViirsHotspots,
    overpass_list: OverpassList,
    jpss_importer: T,
    hs_update_action: HotspotUpdateAction,
    op_update_action: OverpassUpdateAction,
    orbit_calculator: ActorHandle<OrbitActorMsg>
}
 // 3 ops, when last overpass is going to happen ask for new list, then ask for new use job scheduler, 

 // new, init, update
 // actor impl start, execsnapshot action, initialize, update, import error, terminate
impl <T, HotspotUpdateAction, OverpassUpdateAction> JpssImportActor<T, HotspotUpdateAction, OverpassUpdateAction> 
    where T: JpssImporter + Send, 
          HotspotUpdateAction: DataAction<ViirsHotspots>,
          OverpassUpdateAction: DataAction<OverpassList>
{
    pub fn new (config: JpssConfig, jpss_importer:T, hs_update_action: HotspotUpdateAction, op_update_action: OverpassUpdateAction, orbit_calculator:ActorHandle<OrbitActorMsg>) -> Self {
        let hotspots: ViirsHotspots = ViirsHotspots::new(config.satellite.clone(), config.source.clone());
        let overpass_list: OverpassList = OverpassList::new();
        JpssImportActor{source: config.source, satellite: config.satellite, hotspots, overpass_list, jpss_importer, hs_update_action, op_update_action, orbit_calculator}
    }


    pub async fn update_hotspots (&mut self, new_hotspots: ViirsHotspots) {
        self.hotspots.update_hotspots(new_hotspots.clone());
        self.hs_update_action.execute(new_hotspots).await;
    }

    pub async fn update_overpass_list (&mut self, overpass_list: OverpassList) {
        self.overpass_list.update(overpass_list.clone());
        self.op_update_action.execute(overpass_list).await;
    }

    pub fn process_raw_hotspots (&mut self, raw_hotspots:RawHotspots ) -> Result<ViirsHotspots> {
        let hotspots = process_hotspots( raw_hotspots, &self.overpass_list, self.satellite.clone(), self.source.clone())?;
        Ok(hotspots)
    }
}
 
impl_actor! { match msg for Actor< JpssImportActor<T, HotspotUpdateAction, OverpassUpdateAction>, JpssImportActorMsg> 
    where T:JpssImporter + Send + Sync, 
          HotspotUpdateAction: DataAction<ViirsHotspots> + Sync,
          OverpassUpdateAction: DataAction<OverpassList>
    as
    _Start_ => cont! { 
        let hself = self.hself.clone();
        let orbit_calculator =  self.orbit_calculator.clone() ;
        self.jpss_importer.start( hself, orbit_calculator ).await; // move to initialization actor
    }

    ExecSnapshotAction => cont! { msg.0.execute( &self.hotspots).await; }

    UpdateRawHotspots => cont! { self.process_raw_hotspots(msg.0); }

    UpdateHotspots => cont! { self.update_hotspots(msg.0).await; }

    UpdateOverpassList => cont! { self.update_overpass_list(msg.0).await; }

    ImportError => cont! { error!("{:?}", msg.0); }
    
    _Terminate_ => stop! { self.jpss_importer.terminate(); }
}

 // abstraction trait
 pub trait JpssImporter {
    fn start (&mut self, hself: ActorHandle<JpssImportActorMsg>, orbit_handle: ActorHandle<OrbitActorMsg>) -> impl Future<Output=Result<()>> + Send;
    fn terminate (&mut self);
}

 /* #endregion JPSS import actor*/

 /* #region orbit calculator actor *************************************************************************************************/
 
 
#[derive(Debug)] pub struct AskOverpassRequest (pub(crate) OverpassRequest); 

 define_actor_msg_set! { pub OrbitActorMsg = AskOverpassRequest | Query<AskOverpassRequest, UpdateOverpassList> }
// add spec - do not redundently recompute orbits for small areas. May have multiple small areas, should not recompute it 
// large mesoscale region (continental US), keep internal, then get request and filter large orbit to get small portion to return
// initial older overpasses
pub struct OrbitActor <T> 
    where T: OrbitCalculator + Send
{ 
    pub orbit_calculator: T
}

impl <T> OrbitActor <T> 
    where T: OrbitCalculator + Send
{
    pub fn new (orbit_calculator:T) -> Self {
        OrbitActor {orbit_calculator}
    }
}

impl_actor! { match msg for Actor< OrbitActor <T>, OrbitActorMsg> 
    where T: OrbitCalculator + Send
    as
    AskOverpassRequest => cont! {
        let overpass_list_res = self.orbit_calculator.calc_overpass_list(&msg.0);
        match overpass_list_res {
            Ok(overpass_list) => { msg.0.requester.send_msg(UpdateOverpassList(overpass_list)).await.unwrap(); }
            Err(e) => println!("failed to calculate orbit: {}", e),
        }
    }

    Query<AskOverpassRequest, UpdateOverpassList> => cont! {
        let overpass_list_res = self.orbit_calculator.calc_overpass_list(&msg.question.0);
        match overpass_list_res {
            Ok(overpass_list) => match msg.respond(UpdateOverpassList(overpass_list)).await {
                Ok(()) => {},
                Err(e) => println!("failed to send overpasses: {}", e)
            }
            Err(e) => println!("failed to calculate orbit: {}", e)
        }
        
    }
}

 pub trait OrbitCalculator {
    fn calc_overpass_list (&mut self, overpass_request: &OverpassRequest ) -> Result<OverpassList>; // equivalent of JpssActor get_overpasses
 }

 
 /* #endregion orbit calculator actor*/

 /*orbit actor: use query actor example for blue print
 - takes in request message
 - gets overpasses
 - responds with overpasses
  */
