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

use std::sync::Arc;
use odin_jpss::live_importer::{JpssImporterConfig, JpssOrbitCalculatorConfig, LiveJpssConfig, LiveJpssImporter, LiveJpssImporterConfig, LiveJpssOrbitCalculatorConfig, LiveOrbitCalculator};
use odin_jpss::orekit::OverpassList;
use tokio;
use anyhow::Result;
use odin_actor::prelude::*;
use odin_jpss::actor::{JpssConfig, JpssImportActor, JpssImportActorMsg, OrbitActor, OrbitActorMsg, OrbitsReady};
use odin_jpss::{ViirsHotspots, load_config};
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
        println!("------------------------------ overpass update from jpss importer");
        //println!("{}", msg.0) 
    }
}

define_actor_msg_set! { OrbitMonitorMsg = OverpassUpdate}
struct OrbitMonitor {}

impl_actor! { match msg for Actor<OrbitMonitor, OrbitMonitorMsg> as
    OverpassUpdate => cont! { 
        println!("------------------------------ overpass update from orbit calculator");
        //println!("{}", msg.0) 
    }
}


#[tokio::main]
async fn main() -> Result<()>{
    let config: LiveJpssConfig = load_config("jpss_noaa20.ron")?;
    let actor_config: JpssConfig = config.make_jpss_config();
    let arc_config: Arc<LiveJpssConfig> = Arc::new(config);
    let importer_config: JpssImporterConfig = load_config("jpss_noaa20_importer.ron")?;
    let orbit_config: JpssOrbitCalculatorConfig = load_config("jpss_noaa20_orbit.ron")?;

    odin_build::set_bin_context!();

    let mut actor_system = ActorSystem::with_env_tracing("main");

    let hmonitor = spawn_actor!( actor_system, "monitor", JpssMonitor{})?;
    let orbit_hmonitor = spawn_actor!( actor_system, "orbit_monitor", OrbitMonitor{})?;

    let jpss_importer_handle = PreActorHandle::new(&actor_system, "noaa20", 8);
;
    let orbit_actor_handle: ActorHandle<OrbitActorMsg> = spawn_actor!( actor_system, "orbit", OrbitActor::new(
        LiveOrbitCalculator::new(LiveJpssOrbitCalculatorConfig::new( &arc_config, orbit_config)),
        data_action!( jpss_importer_handle.to_actor_handle() : ActorHandle<JpssImportActorMsg> => |data: OrbitsReady| {
            println!("in init action");
            Ok(jpss_importer_handle.try_send_msg(data)?)
        }),
        data_action!( orbit_hmonitor.clone(): ActorHandle<OrbitMonitorMsg> => |data: OverpassList| {
            let msg = OverpassUpdate(data.to_json_pretty().unwrap());
            Ok(orbit_hmonitor.try_send_msg( msg)?)
        }),
    ))?;
    let _actor_handle: ActorHandle<JpssImportActorMsg> = spawn_pre_actor!( actor_system, jpss_importer_handle,  JpssImportActor::new(
        actor_config, 
        LiveJpssImporter::new( LiveJpssImporterConfig::new( &arc_config, importer_config)),
        data_action!( hmonitor.clone(): ActorHandle<JpssMonitorMsg> => |data:ViirsHotspots| {
            let msg = HotspotUpdate(data.to_json_pretty().unwrap());
            Ok(hmonitor.try_send_msg( msg)?)
        }),
        data_action!( hmonitor.clone(): ActorHandle<JpssMonitorMsg> => |data:OverpassList| {
            let msg = OverpassUpdate(data.to_json_pretty().unwrap());
            Ok(hmonitor.try_send_msg( msg)?)
        }),
        orbit_actor_handle
    ))?;


    actor_system.start_all().await?;
    actor_system.process_requests().await;

    Ok(())
}
 