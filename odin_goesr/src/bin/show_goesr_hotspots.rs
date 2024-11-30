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


use tokio;
use anyhow::Result;
use std::any::type_name;

use odin_build;
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_goesr::{
    load_config, GoesrHotspotActor, GoesrHotspotImportActorMsg, GoesrHotspotSet, GoesrHotspotStore, GoesrSat, GoesrService, LiveGoesrHotspotImporter, LiveGoesrHotspotImporterConfig};

 
#[tokio::main]
async fn main()->Result<()> {
    odin_build::set_bin_context!();
    let mut actor_system = ActorSystem::new("main");
    actor_system.request_termination_on_ctrlc();

    let hgoes18 = PreActorHandle::new( &actor_system, "goes18", 8);
    let goes18 = GoesrSat::new( load_config("goes_18.ron")?, hgoes18.to_actor_handle());

    let hgoes16 = PreActorHandle::new( &actor_system, "goes16", 8);
    let goes16 = GoesrSat::new( load_config("goes_16.ron")?, hgoes16.to_actor_handle());

    let hserver = spawn_actor!( actor_system, "server", SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "goesr",
        SpaServiceList::new()
            .add( build_service!( GoesrService::new( vec![goes18,goes16])) )
    ))?;

    let _hgoes18 = spawn_goesr_updater( &mut actor_system, "goes18", hgoes18, load_config( "goes_18_fdcc.ron")?, &hserver)?;
    let _hgoes16 = spawn_goesr_updater( &mut actor_system, "goes16", hgoes16, load_config( "goes_16_fdcc.ron")?, &hserver)?;

    actor_system.timeout_start_all(secs(2)).await?;
    actor_system.process_requests().await?;

    Ok(())
}

fn spawn_goesr_updater (
    actor_system: &mut ActorSystem,
    name: &'static str, 
    pre_handle: PreActorHandle<GoesrHotspotImportActorMsg>, 
    config: LiveGoesrHotspotImporterConfig,
    hserver: &ActorHandle<SpaServerMsg>
) ->OdinActorResult<ActorHandle<GoesrHotspotImportActorMsg>> {
    spawn_pre_actor!( actor_system, pre_handle,  GoesrHotspotActor::new(
        load_config( "goesr.ron")?, 
        LiveGoesrHotspotImporter::new( config),
        dataref_action!{ 
            let hserver: ActorHandle<SpaServerMsg> = hserver.clone(), 
            let name: &'static str = name => 
            |_store:&GoesrHotspotStore| {
                Ok( hserver.try_send_msg( DataAvailable{ sender_id: name, data_type: type_name::<GoesrHotspotStore>()} )? )
            }
        },
        data_action!{ 
            let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => 
            |hotspots:GoesrHotspotSet| {
                //let data = ws_msg!("odin_goesr/odin_goesr.js",hotspots).to_json()?;
                let data = WsMsg::json( GoesrService::mod_path(), "hotspots", hotspots)?;
                Ok( hserver.try_send_msg( BroadcastWsMsg{data})? )
            }
        },
    ))
}