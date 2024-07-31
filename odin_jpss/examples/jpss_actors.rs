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

 //! example application of how to create and use a [JpssHotspotImportActor] in a standalone, configured executable.
 
use odin_jpss::live_importer::LiveOrbitCalculator;
use odin_jpss::orekit::OverpassList;
use tokio;
use anyhow::Result;
use odin_actor::prelude::*;
use odin_jpss::actor::{JpssImportActor, OrbitActor};
use odin_jpss::{live_importer::LiveJpssImporter, ViirsHotspots, load_config};
use odin_build;

#[derive(Debug)] pub struct HotspotUpdate(String);
#[derive(Debug)] pub struct OverpassUpdate(String);

define_actor_msg_set! { JpssMonitorMsg = HotspotUpdate | OverpassUpdate}
struct JpssMonitor {}

impl_actor! { match msg for Actor<JpssMonitor, JpssMonitorMsg> as
    HotspotUpdate => cont! { 
        println!("------------------------------ hotspot update");
        println!("{}", msg.0) 
    }
    OverpassUpdate => cont! { 
        println!("------------------------------ overpass update");
        println!("{}", msg.0) 
    }
}

 #[tokio::main]
async fn main() -> Result<()>{
    odin_build::set_bin_context!();

    let mut actor_system = ActorSystem::with_env_tracing("main");

    let hmonitor = spawn_actor!( actor_system, "monitor", JpssMonitor{})?;

    let orbit_actor_handle = spawn_actor!( actor_system, "orbit", OrbitActor::new(
        LiveOrbitCalculator {}
    ))?;

    let _actor_handle = spawn_actor!( actor_system, "jpss",  JpssImportActor::new(
        load_config( "jpss.ron")?, 
        LiveJpssImporter::new( load_config( "jpss_noaa20.ron")?),
        data_action!( hmonitor.clone(): ActorHandle<JpssMonitorMsg> => |data:ViirsHotspots| {
            let msg = HotspotUpdate(data.to_json_pretty().unwrap());
            hmonitor.try_send_msg( msg)
        }),
        data_action!( hmonitor.clone(): ActorHandle<JpssMonitorMsg> => |data:OverpassList| {
            let msg = OverpassUpdate(data.to_json_pretty().unwrap());
            hmonitor.try_send_msg( msg)
        }),
        orbit_actor_handle
    ))?;


    actor_system.start_all().await?;
    actor_system.process_requests().await;

    Ok(())
}
 