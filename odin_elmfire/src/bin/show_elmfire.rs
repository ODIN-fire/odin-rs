/*
 * Copyright 2026 Peter Mehlitz
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *    http://apache.org
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

#![allow(unused)]

use std::{sync::Arc, fs::remove_dir_all};
use odin_actor::prelude::*;
use odin_server::prelude::*;

use odin_elmfire::{ElmfireRun, ElmfireService, actor::{ElmfireActor,ElmfireActorMsg, server_update_action, server_remove_action}, load_config};

run_actor_system!( actor_system => {
    let pre_fire = PreActorHandle::new( &actor_system, "elmfire", 8);

    let hserver = spawn_actor!( actor_system, "server",
        SpaServer::new(
            odin_server::load_config("spa_server.ron")?,
            "elmfire",
            SpaServiceList::new()
                .add( build_service!( let hfire: ActorHandle<ElmfireActorMsg> = pre_fire.to_actor_handle() => ElmfireService::new( hfire) ))
        )
    )?;

    let hfire = spawn_pre_actor!( actor_system, pre_fire,
        ElmfireActor::new(
            load_config("elmfire.ron")?,
            server_update_action( hserver.clone()),
            server_remove_action( hserver.clone())
        )
    )?;

    Ok(())
});
