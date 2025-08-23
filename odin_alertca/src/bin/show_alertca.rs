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

use std::sync::Arc;
use odin_common::json_writer::{JsonWritable,JsonWriter};
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_alertca::{
    actor::AlertCaActor, 
    alertca_service::AlertCaService, 
    live_connector::LiveAlertCaConnector, 
    load_config, CameraStore, CameraUpdate, get_json_update_msg
};
use anyhow::Result;

run_actor_system!( actor_system => {
    let pre_aca = PreActorHandle::new( &actor_system, "alertca", 8);

    let haca = pre_aca.to_actor_handle();
    let hserver = spawn_actor!( actor_system, "server", 
        SpaServer::new(
            odin_server::load_config("spa_server.ron")?,
            "alert-ca",
            SpaServiceList::new()
                .add( build_service!( => AlertCaService::new( haca) ))
        )
    )?;

    let aca_id = pre_aca.get_id();
    let haca = spawn_pre_actor!( actor_system, pre_aca,
        AlertCaActor::new( 
            load_config("sf_bay_area.ron")?,
            LiveAlertCaConnector::new,
            dataref_action!( 
                let sender_id: Arc<String> = aca_id, 
                let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => |store: &CameraStore| {
                    Ok( hserver.try_send_msg( DataAvailable::new::<CameraStore>(sender_id) )? )
                }
            ),
            dataref_action!( 
                let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => |updates: &Vec<CameraUpdate>| {
                    let ws_msg = get_json_update_msg( &updates);
                    Ok( hserver.try_send_msg( BroadcastWsMsg{ws_msg})? )
                }
            )
        )
    )?;

    Ok(())
});