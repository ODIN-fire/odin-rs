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

#![allow(unused)]

//! example application of how to create and use a [GoesRHotspotImportActor] in a standalone, configured executable.

use tokio;
use anyhow::Result;
use odin_actor::prelude::*;
use odin_goesr::actor::{self, GoesrHotspotActor};
use odin_goesr::{live_importer::LiveGoesrHotspotImporter, GoesrHotspotStore, GoesrHotspotSet, load_config};
use odin_build;

#[derive(Debug)] pub struct Update(String);

define_actor_msg_set! { GoesrMonitorMsg = Update }
struct GoesrMonitor {}

impl_actor! { match msg for Actor<GoesrMonitor,GoesrMonitorMsg> as
    Update => cont! { 
        println!("------------------------------ update");
        println!("{}", msg.0) 
    }
}


#[tokio::main]
async fn main() -> Result<()>{
    odin_build::set_bin_context!();

    let mut actor_system = ActorSystem::with_env_tracing("main");
    actor_system.request_termination_on_ctrlc(); // don't just kill the process - we might be in the middle of retrieving AWS data

    let hmonitor = spawn_actor!( actor_system, "monitor", GoesrMonitor{})?;

    let _actor_handle = spawn_actor!( actor_system, "goesr",  GoesrHotspotActor::new(
        load_config( "goesr.ron")?, 
        LiveGoesrHotspotImporter::new( load_config( "goes_18_fdcc.ron")?),
        dataref_action!( let hmonitor: ActorHandle<GoesrMonitorMsg> = hmonitor.clone() => |store: &GoesrHotspotStore| {
            for hs in store.iter_old_to_new(){
                let msg = Update(hs.to_json_pretty().unwrap());
                hmonitor.try_send_msg(msg);
            }
            Ok(())
        }),
        data_action!( let hmonitor: ActorHandle<GoesrMonitorMsg> = hmonitor => |hs:GoesrHotspotSet| {
            let msg = Update(hs.to_json_pretty().unwrap());
            Ok( hmonitor.try_send_msg( msg)? )
        }),
    ))?;


    actor_system.start_all().await?;
    actor_system.process_requests().await;

    Ok(())
}