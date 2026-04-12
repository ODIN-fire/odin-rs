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
use std::sync::Arc;

use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_fems::{ load_config, FemsStore, FemsStation, actor::{FemsActor,FemsActorMsg}, service::FemsService };

run_actor_system!( actor_system => {
    let pre_fems = PreActorHandle::new( &actor_system, "fems", 8);

    let hserver = spawn_actor!( actor_system, "server",
        SpaServer::new(
            odin_server::load_config("spa_server.ron")?,
            "fems",
            SpaServiceList::new()
                .add( build_service!( let hfems: ActorHandle<FemsActorMsg> = pre_fems.to_actor_handle() => FemsService::new( hfems) ))
        )
    )?;

    let fems_id = pre_fems.get_id();
    let _hfems = spawn_pre_actor!( actor_system, pre_fems,
        FemsActor::new(
            load_config("fems.ron")?,
            dataref_action!(
                let sender_id: Arc<String> = fems_id,
                let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => |store: &FemsStore| {
                    Ok( hserver.try_send_msg( DataAvailable::new::<FemsStore>(sender_id) )? )
                }
            ),
            dataref_action!(
                let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => |station: &FemsStation| {
                    let ws_msg = station.get_json_update_msg();
                    Ok( hserver.try_send_msg( BroadcastWsMsg{ws_msg})? )
                }
            )
        )
    )?;

    Ok(())
});
