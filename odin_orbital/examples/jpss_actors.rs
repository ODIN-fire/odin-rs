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
use odin_orbital::live_importer::{LiveOrbitalSatConfig, LiveOrbitalSatImporter, LiveOrbitalSatImporterConfig, LiveOrbitalSatOrbitCalculatorConfig, LiveOrbitCalculator};
use odin_orbital::overpass::OverpassList;
use tokio;
use anyhow::Result;
use odin_actor::prelude::*;
use odin_orbital::actor::{OrbitalSatConfig, OrbitalSatImporterConfig, OrbitalSatOrbitCalculatorConfig, OrbitalSatImportActor, OrbitalSatImportActorMsg, OrbitActor, OrbitActorMsg, OrbitsReady};
use odin_orbital::{load_config, ViirsHotspotSet, ViirsHotspotStore};
use odin_build;

#[derive(Debug)] pub struct InitUpdate(String);
#[derive(Debug)] pub struct HotspotUpdate(String);
#[derive(Debug)] pub struct OverpassUpdate(String);

define_actor_msg_set! { OrbitalSatMonitorMsg = InitUpdate | HotspotUpdate | OverpassUpdate}
struct OrbitalSatMonitor {}

impl_actor! { match msg for Actor<OrbitalSatMonitor, OrbitalSatMonitorMsg> as
    InitUpdate => cont! { 
        println!("------------------------------ init hotspots");
        println!("{}", msg.0) 
    }
    HotspotUpdate => cont! { 
        println!("------------------------------ hotspot update");
        println!("{}", msg.0) 
    }
    OverpassUpdate => cont! { 
        println!("------------------------------ overpass update from polar sat importer");
        //println!("{}", msg.0) 
    }
}


#[tokio::main]
async fn main() -> Result<()>{
    let config: LiveOrbitalSatConfig = load_config("jpss_noaa20.ron")?;
    let actor_config: OrbitalSatConfig = config.make_orbital_sat_config();
    let arc_config: Arc<LiveOrbitalSatConfig> = Arc::new(config);
    let importer_config: OrbitalSatImporterConfig = load_config("jpss_noaa20_importer.ron")?;
    let orbit_config: OrbitalSatOrbitCalculatorConfig = load_config("jpss_noaa20_orbit.ron")?;

    odin_build::set_bin_context!();

    let mut actor_system = ActorSystem::with_env_tracing("main");

    let hmonitor = spawn_actor!( actor_system, "monitor", OrbitalSatMonitor{})?;

    let jpss_importer_handle = PreActorHandle::new(&actor_system, "noaa20", 8);

    let orbit_actor_handle: ActorHandle<OrbitActorMsg> = spawn_actor!( actor_system, "orbit", OrbitActor::new(
        LiveOrbitCalculator::new(LiveOrbitalSatOrbitCalculatorConfig::new( &arc_config, orbit_config)),
        data_action!( let jpss_importer_handle: ActorHandle<OrbitalSatImportActorMsg>  = jpss_importer_handle.to_actor_handle() => |data: OrbitsReady| {
            Ok(jpss_importer_handle.try_send_msg(data)?)
        })
    ))?;
    let _actor_handle: ActorHandle<OrbitalSatImportActorMsg> = spawn_pre_actor!( actor_system, jpss_importer_handle,  OrbitalSatImportActor::new(
        actor_config, 
        LiveOrbitalSatImporter::new( LiveOrbitalSatImporterConfig::new( &arc_config, importer_config)),
        dataref_action!{ 
            let hmonitor: ActorHandle<OrbitalSatMonitorMsg> = hmonitor.clone() => 
            |store:&ViirsHotspotStore| {
                let msg = format!("initial hotspots ready, {:?} hotspots", store.to_hotspots().len()).to_string();
                Ok(hmonitor.try_send_msg(InitUpdate(msg))?)   
            }
        },
        data_action!{  
            let hmonitor: ActorHandle<OrbitalSatMonitorMsg> = hmonitor.clone() => 
            |data:ViirsHotspotSet| {
                let msg = HotspotUpdate(data.to_json_pretty().unwrap());
                Ok(hmonitor.try_send_msg( msg)?)
            }
        },
        data_action!{ let hmonitor: ActorHandle<OrbitalSatMonitorMsg> = hmonitor.clone() => 
            |data:OverpassList| {
                let msg = OverpassUpdate(data.to_json_pretty().unwrap());
                Ok(hmonitor.try_send_msg( msg)?)
            }
        },
        orbit_actor_handle
    ))?;


    actor_system.start_all().await?;
    actor_system.process_requests().await;

    Ok(())
}
 