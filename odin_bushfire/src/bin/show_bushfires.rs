/*
 * Copyright © 2026, United States Government, as represented by the Administrator of
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
use odin_actor::prelude::*;
use odin_server::prelude::*;

use odin_bushfire::{Bushfire, BushfireService, BushfireStore, actor::{BushfireActor,BushfireActorMsg}, load_config, get_json_update_msg};

run_actor_system!( actor_system => {
    let pre_fire = PreActorHandle::new( &actor_system, "bushfires", 8);

    let hserver = spawn_actor!( actor_system, "server",
        SpaServer::new(
            odin_server::load_config("spa_server.ron")?,
            "bushfire",
            SpaServiceList::new()
                .add( build_service!( let hfire: ActorHandle<BushfireActorMsg> = pre_fire.to_actor_handle() => BushfireService::new( hfire) ))
        )
    )?;

    let fire_id = pre_fire.get_id();
    let hfire = spawn_pre_actor!( actor_system, pre_fire,
        BushfireActor::new(
            load_config("bushfire.ron")?,
            dataref_action!(
                let sender_id: Arc<String> = fire_id,
                let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => |store: &BushfireStore| {
                    Ok( hserver.try_send_msg( DataAvailable::new::<BushfireStore>(sender_id) )? )
                }
            ),
            data_action!(
                let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => |updates: Vec<Bushfire>| {
                    let ws_msg = get_json_update_msg( &updates);
                    Ok( hserver.try_send_msg( BroadcastWsMsg{ws_msg})? )
                }
            )
        )
    )?;

    Ok(())
});
