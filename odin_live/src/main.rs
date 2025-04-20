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
use odin_goesr::{GoesrHotspotService, actor::spawn_goesr_hotspot_actors};
use odin_share::prelude::*;
use odin_sentinel::{SentinelStore, SentinelUpdate, LiveSentinelConnector, SentinelActor, sentinel_service::SentinelService};


run_actor_system!( actor_system => {
 
    let pre_server = PreActorHandle::new( &actor_system, "server", 64);

    //--- spawn the shared store actor
    let store_name = Arc::new("shared".to_string());
    let hstore = spawn_actor!( actor_system, &store_name, new_shared_store_actor( load_store()?, store_name.clone(), pre_server.to_actor_handle()))?;

    //--- spawn the shared store actor
    let sat_names = vec![ "goes_18", "goes_16" ];
    let sats = spawn_goesr_hotspot_actors( &mut actor_system, pre_server.to_actor_handle(), &sat_names, "fdcc")?;

    //--- spawn the sentinel actor
    let sentinel_name = Arc::new("sentinel".to_string());
    let hsentinel = spawn_actor!( actor_system, &sentinel_name, SentinelActor::new( 
        LiveSentinelConnector::new( odin_sentinel::load_config( "sentinel.ron")?), 
        dataref_action!( 
            let hserver: ActorHandle<SpaServerMsg> = pre_server.to_actor_handle(),
            let sender_id: Arc<String> = sentinel_name.clone() =>
            |_store: &SentinelStore| {
                Ok( hserver.try_send_msg( DataAvailable::new::<SentinelStore>(sender_id) )? )
            }
        ), 
        data_action!( 
            let hserver: ActorHandle<SpaServerMsg> = pre_server.to_actor_handle() => 
            |update:SentinelUpdate| {
                let ws_msg = WsMsg::json( SentinelService::mod_path(), "update", update)?;
                Ok( hserver.try_send_msg( BroadcastWsMsg{ws_msg})? )
            }
        )
    ))?;

    //--- finally spawn the server actor
    let _hserver = spawn_pre_actor!( actor_system, pre_server, SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "live",
        SpaServiceList::new()
            .add( build_service!( => GoesrHotspotService::new( sats)) )
            .add( build_service!( let hsentinel = hsentinel.clone() => SentinelService::new( hsentinel)))
            .add( build_service!( let hstore = hstore.clone() => ShareService::new( hstore)))
    ))?;

    Ok(())
});


fn load_store()->OdinActorResult<PersistentHashMapStore<SharedItemType>> {
    // FIXME - this should come from the global <ODIN-ROOT>/data/odin_live/ dir
    PersistentHashMapStore::new( &"data/shared_items.json", false).map_err(|e| op_failed(e.to_string()))
}