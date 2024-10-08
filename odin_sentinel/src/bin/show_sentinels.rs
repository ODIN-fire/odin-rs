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
use odin_sentinel::{SentinelStore,SentinelUpdate,LiveSentinelConnector,SentinelActor,load_config, web::SentinelService};


run_actor_system!( actor_system => {

    let hsentinel = PreActorHandle::new( &actor_system, "updater", 8);

    let hserver = spawn_actor!( actor_system, "server", SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "sentinels",
        SpaServiceList::new()
            .add( build_service!( hsentinel.to_actor_handle() => SentinelService::new( hsentinel)))
    ))?;

    let _hsentinel = spawn_pre_actor!( actor_system, hsentinel, SentinelActor::new(
        LiveSentinelConnector::new( load_config( "sentinel.ron")?), 
        dataref_action!( hserver.clone(): ActorHandle<SpaServerMsg> => |_store: &SentinelStore| {
            // we could directly send a BroadcastWsMsg here but if there are no connections yet that would 
            // create a potentially large WsMsg for naught
            Ok( hserver.try_send_msg( DataAvailable{sender_id:"updater",data_type: type_name::<SentinelStore>()} )? )
        }),
        data_action!( hserver: ActorHandle<SpaServerMsg> => |update:SentinelUpdate| {
            let data = ws_msg!("odin_sentinel/odin_sentinel.js",update).to_json()?;
            Ok( hserver.try_send_msg( BroadcastWsMsg{data})? )
        }),
    ))?;
    
    Ok(())
});