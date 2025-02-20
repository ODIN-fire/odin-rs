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

use std::sync::Arc;
use chrono::Utc;
use odin_orbital::live_importer::{LiveOrbitalSatImporterConfig, LiveOrbitalSatConfig, LiveOrbitalSatOrbitCalculatorConfig, LiveOrbitalSatImporter, LiveOrbitCalculator};
use tokio;
use anyhow::Result;

use odin_build;
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_orbital::actor::{OrbitalSatConfig, OrbitalSatImporterConfig, OrbitalSatOrbitCalculatorConfig, OrbitalSatImportActor, OrbitalSatImportActorMsg, OrbitActor, OrbitActorMsg, OrbitsReady};
use odin_orbital::{load_config, OrbitalSat, ViirsHotspotSet, ViirsHotspotStore};
use odin_orbital::orekit::OverpassList;
use odin_orbital::orbital_service::OrbitalSatService;


#[tokio::main]
async fn main() -> Result<()>{
    // config loading - should simplify this
    let config: LiveOrbitalSatConfig = load_config("jpss_noaa20.ron")?;
    let actor_config: OrbitalSatConfig = config.make_orbital_sat_config();
    let arc_config: Arc<LiveOrbitalSatConfig> = Arc::new(config);
    let importer_config: OrbitalSatImporterConfig = load_config("jpss_noaa20_importer.ron")?;
    let orbit_config: OrbitalSatOrbitCalculatorConfig = load_config("jpss_noaa20_orbit.ron")?;
    let live_orbit_config_noaa20: LiveOrbitalSatOrbitCalculatorConfig = LiveOrbitalSatOrbitCalculatorConfig::new( &arc_config, orbit_config);
    let live_importer_config_noaa20: LiveOrbitalSatImporterConfig = LiveOrbitalSatImporterConfig::new( &arc_config, importer_config);
    
    // actor system initialization 
    odin_build::set_bin_context!();

    let mut actor_system = ActorSystem::with_env_tracing("main");

    let hnoaa20 = PreActorHandle::new(&actor_system, "J-1", 8);
    let noaa20 = OrbitalSat::new(load_config("noaa20.ron")?, hnoaa20.to_actor_handle());
    let noaa20_name: &'static str = "J-1";
    let hserver = spawn_actor!( actor_system, "server", SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "jpss",
        SpaServiceList::new()
            .add( build_service!( => OrbitalSatService::new( vec![noaa20])) )
    ))?;

    let horbit: ActorHandle<OrbitActorMsg> = spawn_orbital_sat_orbit_calculator(&mut actor_system, noaa20_name, hnoaa20.to_actor_handle(), live_orbit_config_noaa20)?;
    let _hnoaa20: ActorHandle<OrbitalSatImportActorMsg> = spawn_orbital_sat_importer(&mut actor_system, hnoaa20, horbit, actor_config, live_importer_config_noaa20, &hserver)?;

    // run actors
    actor_system.start_all().await?;
    actor_system.process_requests().await;

    Ok(())
}

fn spawn_orbital_sat_orbit_calculator (
    actor_system: &mut ActorSystem,
    name: &'static str, 
    importer_handle: ActorHandle<OrbitalSatImportActorMsg>, 
    config: LiveOrbitalSatOrbitCalculatorConfig
) -> OdinActorResult<ActorHandle<OrbitActorMsg>> {
    spawn_actor!( actor_system, name, OrbitActor::new(
        LiveOrbitCalculator::new(config),
        data_action!( 
            let jpss_importer_handle: ActorHandle<OrbitalSatImportActorMsg>  = importer_handle => |data: OrbitsReady| {
            Ok(jpss_importer_handle.try_send_msg(data)?)
        }) // sends orbit ready to importer
    ))
}
fn spawn_orbital_sat_importer (
    actor_system: &mut ActorSystem,
    pre_handle: PreActorHandle<OrbitalSatImportActorMsg>, 
    orbit_handle: ActorHandle<OrbitActorMsg>,
    actor_config: OrbitalSatConfig,
    config: LiveOrbitalSatImporterConfig,
    hserver: &ActorHandle<SpaServerMsg>
) -> OdinActorResult<ActorHandle<OrbitalSatImportActorMsg>> {
    let init_action = dataref_action!{ 
        let hserver: ActorHandle<SpaServerMsg> = hserver.clone(), 
        let sender_id: Arc<String> = pre_handle.get_id().clone() => 
        |_store:&ViirsHotspotStore| {
            Ok( hserver.try_send_msg( DataAvailable::new::<ViirsHotspotStore>(sender_id) )? )
        }
    };
    let overpass_update_action = data_action!{ 
        let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => |data:OverpassList| {
            for overpass in data.overpasses.iter(){
                let data = WsMsg::json( OrbitalSatService::mod_path(), "overpass", overpass)?;
                hserver.try_send_msg( BroadcastWsMsg{data})?;
            }
            Ok(())
    }};
    let hotspot_update_action = data_action!{ 
        let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => |data:ViirsHotspotSet| {
            let data = WsMsg::json( OrbitalSatService::mod_path(), "hotspots", data)?;
            Ok( hserver.try_send_msg( BroadcastWsMsg{data})? )
    }};
    spawn_pre_actor!( actor_system, pre_handle,  OrbitalSatImportActor::new(
        actor_config, 
        LiveOrbitalSatImporter::new( config ),
        init_action,
        hotspot_update_action,
        overpass_update_action,
        orbit_handle
    ))
}