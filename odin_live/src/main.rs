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

use std::sync::Arc;
 
use odin_build;
use odin_actor::{errors::op_failed, prelude::*};
use odin_server::prelude::*;
use odin_goesr::{
    LiveGoesrHotspotImporter, LiveGoesrHotspotImporterConfig,  
    GoesrHotspotStore, GoesrHotspotSet, GoesrHotspotActor, GoesrHotspotImportActorMsg, GoesrSat, GoesrService
};
use odin_share::prelude::*;
use odin_sentinel::{SentinelStore, SentinelUpdate, LiveSentinelConnector, SentinelActor, sentinel_service::SentinelService};


run_actor_system!( actor_system => {
 
    //--- (1a) set up GOES-R data source handles
    let hgoes18 = PreActorHandle::new( &actor_system, "goes18", 8);
    let goes18 = GoesrSat::new( odin_goesr::load_config("goes_18.ron")?, hgoes18.to_actor_handle());
 
    let hgoes16 = PreActorHandle::new( &actor_system, "goes16", 8);
    let goes16 = GoesrSat::new( odin_goesr::load_config("goes_16.ron")?, hgoes16.to_actor_handle());
 
    //--- (1b) set up Sentinel data source handles
    let pre_sentinel = PreActorHandle::new( &actor_system, "sentinel", 8);

    //--- (1c) the future store actor for SharedItems (user entered data)
    let pre_store = PreActorHandle::<SharedStoreActorMsg<SharedItemType>>::new( &actor_system, "store", 8);

    //--- (2) spawn the server actor
    let hserver = spawn_actor!( actor_system, "server", SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "live",
        SpaServiceList::new()
            .add( build_service!( => GoesrService::new( vec![goes18,goes16])) )
            .add( build_service!( let hsentinel = pre_sentinel.to_actor_handle() => SentinelService::new( hsentinel)))
            .add( build_service!( let hstore = pre_store.to_actor_handle() => ShareService::new( hstore)))
    ))?;
 
    //--- (3) spawn the shared store actor
    let store_actor = new_shared_store_actor( load_store()?, pre_store.get_id(), &hserver);
    let _hstore = spawn_pre_actor!( actor_system, pre_store, store_actor)?;

    //--- (4) spawn the data source actors we did set up in (1) 
    let _hgoes18 = spawn_goesr_updater( &mut actor_system, hgoes18, odin_goesr::load_config( "goes_18_fdcc.ron")?, &hserver)?;
    let _hgoes16 = spawn_goesr_updater( &mut actor_system, hgoes16, odin_goesr::load_config( "goes_16_fdcc.ron")?, &hserver)?;
 
    let init_action = dataref_action!( 
        let hserver: ActorHandle<SpaServerMsg> = hserver.clone(),
        let sender_id: Arc<String> = pre_sentinel.get_id() =>
        |_store: &SentinelStore| {
            Ok( hserver.try_send_msg( DataAvailable::new::<SentinelStore>(sender_id) )? )
        }
    );
    let update_action = data_action!( let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => |update:SentinelUpdate| {
        //let data = ws_msg!("odin_sentinel/odin_sentinel.js",update).to_json()?;
        let data = WsMsg::json( SentinelService::mod_path(), "update", update)?;
        Ok( hserver.try_send_msg( BroadcastWsMsg{data})? )
    });
    let connector = LiveSentinelConnector::new( odin_sentinel::load_config( "sentinel.ron")?);

    let _hsentinel = spawn_pre_actor!( actor_system, pre_sentinel, 
        SentinelActor::new( connector, init_action, update_action)
    )?;

    Ok(())
});
 
fn spawn_goesr_updater (
    actor_system: &mut ActorSystem,
    pre_handle: PreActorHandle<GoesrHotspotImportActorMsg>, 
    config: LiveGoesrHotspotImporterConfig,
    hserver: &ActorHandle<SpaServerMsg>
) ->OdinActorResult<ActorHandle<GoesrHotspotImportActorMsg>> 
{
    let init_action = dataref_action!{ 
        let hserver: ActorHandle<SpaServerMsg> = hserver.clone(), 
        let sender_id: Arc<String> = pre_handle.get_id() => 
        |_store:&GoesrHotspotStore| {
            Ok( hserver.try_send_msg( DataAvailable::new::<GoesrHotspotStore>(sender_id) )? )
        }
    };
    
    let update_action = data_action!{ 
        let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => 
        |hotspots:GoesrHotspotSet| {
            let data = WsMsg::json( GoesrService::mod_path(), "hotspots", hotspots)?;
            Ok( hserver.try_send_msg( BroadcastWsMsg{data})? )
        }
    };

    spawn_pre_actor!( actor_system, pre_handle,  
        GoesrHotspotActor::new( odin_goesr::load_config( "goesr.ron")?, LiveGoesrHotspotImporter::new(config), init_action, update_action)
    )
}

fn load_store()->OdinActorResult<PersistentHashMapStore<SharedItemType>> {
    // FIXME - this should come from the global <ODIN-ROOT>/data/odin_live/ dir
    PersistentHashMapStore::new( &"data/shared_items.json", false).map_err(|e| op_failed(e.to_string()))
}