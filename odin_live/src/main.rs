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

use std::any::type_name;
 
use odin_build;
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_goesr::{
    LiveGoesrHotspotImporter, LiveGoesrHotspotImporterConfig,  
    GoesrHotspotStore, GoesrHotspotSet, GoesrHotspotActor, GoesrHotspotImportActorMsg, GoesrSat, GoesrService
};

use odin_sentinel::{SentinelStore, SentinelUpdate, LiveSentinelConnector, SentinelActor, sentinel_service::SentinelService};


run_actor_system!( actor_system => {
 
    //--- (1a) set up GOES-R data source handles
    let hgoes18 = PreActorHandle::new( &actor_system, "goes18", 8);
    let goes18 = GoesrSat::new( odin_goesr::load_config("goes_18.ron")?, hgoes18.to_actor_handle());
 
    let hgoes16 = PreActorHandle::new( &actor_system, "goes16", 8);
    let goes16 = GoesrSat::new( odin_goesr::load_config("goes_16.ron")?, hgoes16.to_actor_handle());
 
    //--- (1b) set up Sentinel data source handles
    let hsentinel = PreActorHandle::new( &actor_system, "sentinel", 8);

    //--- (2) spawn the server actor
    let hserver = spawn_actor!( actor_system, "server", SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "live",
        SpaServiceList::new()
            .add( build_service!( GoesrService::new( vec![goes18,goes16])) )
            .add( build_service!( hsentinel.to_actor_handle() => SentinelService::new( hsentinel)))
    ))?;
 
    //--- (3) spawn the data source actors we did set up in (1) 
    let _hgoes18 = spawn_goesr_updater( &mut actor_system, "goes18", hgoes18, odin_goesr::load_config( "goes_18_fdcc.ron")?, &hserver)?;
    let _hgoes16 = spawn_goesr_updater( &mut actor_system, "goes16", hgoes16, odin_goesr::load_config( "goes_16_fdcc.ron")?, &hserver)?;
 
    let _hsentinel = spawn_pre_actor!( actor_system, hsentinel, SentinelActor::new(
        LiveSentinelConnector::new( odin_sentinel::load_config( "sentinel.ron")?), 
        dataref_action!( let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => |_store: &SentinelStore| {
            Ok( hserver.try_send_msg( DataAvailable{sender_id:"sentinel",data_type: type_name::<SentinelStore>()} )? )
        }),
        data_action!( let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => |update:SentinelUpdate| {
            //let data = ws_msg!("odin_sentinel/odin_sentinel.js",update).to_json()?;
            let data = WsMsg::json( SentinelService::mod_path(), "update", update)?;
            Ok( hserver.try_send_msg( BroadcastWsMsg{data})? )
        }),
        no_data_action() // we do client side inactive checks
    ))?;

    Ok(())
});
 
fn spawn_goesr_updater (
    actor_system: &mut ActorSystem,
    name: &'static str, 
    pre_handle: PreActorHandle<GoesrHotspotImportActorMsg>, 
    config: LiveGoesrHotspotImporterConfig,
    hserver: &ActorHandle<SpaServerMsg>
) ->OdinActorResult<ActorHandle<GoesrHotspotImportActorMsg>> {
    spawn_pre_actor!( actor_system, pre_handle,  GoesrHotspotActor::new(
        odin_goesr::load_config( "goesr.ron")?, 
        LiveGoesrHotspotImporter::new( config),
        dataref_action!{
            let hserver: ActorHandle<SpaServerMsg> = hserver.clone(), 
            let name: &'static str = name => 
            |_store:&GoesrHotspotStore| {
                Ok( hserver.try_send_msg( DataAvailable{ sender_id: name, data_type: type_name::<GoesrHotspotStore>()} )? )
            }
        },
        data_action!( let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => |hotspots:GoesrHotspotSet| {
            //let data = ws_msg!("odin_goesr/odin_goesr.js",hotspots).to_json()?;
            let data = WsMsg::json( GoesrService::mod_path(), "hotspots", hotspots)?;
            Ok( hserver.try_send_msg( BroadcastWsMsg{data})? )
        }),
    ))
}