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

use std::sync::Arc;
use odin_build;
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_himawari::{
    HimawariConfig, HimawariHotspotStore, HimawariHotspotSet, PKG_CACHE_DIR,
    service::HimawariHotspotService, actor::HimawariHotspotActor, live_importer::LiveHimawariHotspotImporter
};

run_actor_system!( actor_system => {

    let pre_server = PreActorHandle::new( &actor_system, "server", 64);

    let config: Arc<HimawariConfig> = Arc::new( odin_himawari::load_config("himawari.ron")?);
    let himawari = spawn_actor!( actor_system, "himawari", HimawariHotspotActor::new(
        config.clone(),
        LiveHimawariHotspotImporter::new( config, Arc::new( PKG_CACHE_DIR.clone())),
        dataref_action!(
            let sender_id: Arc<String> = Arc::new("himawari".to_string()),
            let hserver: ActorHandle<SpaServerMsg> = pre_server.to_actor_handle() => |store: &HimawariHotspotStore| {
                Ok( hserver.try_send_msg( DataAvailable::new::<HimawariHotspotStore>(sender_id) )? )
            }
        ),
        data_action!(
            let hserver: ActorHandle<SpaServerMsg> = pre_server.to_actor_handle() => |hs: HimawariHotspotSet| {
                let w = hs.to_json()?;
                let ws_msg = ws_msg_from_json( HimawariHotspotService::mod_path(), "hotspots", w.as_str());
                Ok( hserver.try_send_msg( BroadcastWsMsg{ws_msg})? )
            }
        )
    ))?;

    let _hserver = spawn_pre_actor!( actor_system, pre_server,
        SpaServer::new(
            odin_server::load_config("spa_server.ron")?,
            "himawari",
            SpaServiceList::new()
                .add( build_service!( => HimawariHotspotService::new( himawari) ))
        )
    )?;

    Ok(())
});
