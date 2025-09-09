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

use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_n5::{
    actor::N5Actor, get_json_update_msg, get_n5_devices, live_connector::LiveN5Connector, 
    load_config, Device, N5Config, N5DataUpdate, N5DeviceStore, n5_service::N5Service,
};

run_actor_system!( actor_system => {
    let pre_n5 = PreActorHandle::new( &actor_system, "n5", 8);

    let hn5 = pre_n5.to_actor_handle();
    let hserver = spawn_actor!( actor_system, "server", 
        SpaServer::new(
            odin_server::load_config("spa_server.ron")?,
            "n5",
            SpaServiceList::new()
                .add( build_service!( => N5Service::new( hn5) ))
        )
    )?;


    let n5_id = pre_n5.get_id();
    let hn5 = spawn_pre_actor!( actor_system, pre_n5, 
        N5Actor::new( 
            LiveN5Connector::new( load_config("n5.ron")?),
            dataref_action!( 
                let sender_id: Arc<String> = n5_id, 
                let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => |store: &N5DeviceStore| {
                    Ok( hserver.try_send_msg( DataAvailable::new::<N5DeviceStore>(sender_id) )? )
                }
            ),
            data_action!( 
                let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => |updates: Vec<N5DataUpdate>| {
                    let ws_msg = get_json_update_msg( &updates);
                    Ok( hserver.try_send_msg( BroadcastWsMsg{ws_msg})? )
                }
            )
        )
    )?;

    Ok(())
});