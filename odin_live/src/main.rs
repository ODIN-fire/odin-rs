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
 
use odin_build;
use odin_common::arc;
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_goesr::{GoesrHotspotService, actor::spawn_goesr_hotspot_actors};
use odin_orbital::{init_orbital_data,actor::spawn_orbital_hotspot_actors,hotspot_service::OrbitalHotspotService};
use odin_share::prelude::*;
use odin_geolayer::GeoLayerService;
use odin_sentinel::{SentinelStore, SentinelUpdate, LiveSentinelConnector, SentinelActor, sentinel_service::SentinelService};


run_actor_system!( actor_system => {
    // make sure our orbit calculation uses up-to-date ephemeris
    init_orbital_data()?;

    let pre_server = PreActorHandle::new( &actor_system, "server", 64);

    // we would normally initialize the store via default_shared_items() but those normally reside outside the repository
    let hstore = spawn_server_share_actor(&mut actor_system, "share", pre_server.to_actor_handle(), default_shared_items(), false)?;

    //--- spawn the GOES-R actors
    let goesr_sat_configs = vec![ "goes_18.ron", "goes_19.ron" ];
    let goesr_sats = spawn_goesr_hotspot_actors( &mut actor_system, pre_server.to_actor_handle(), &goesr_sat_configs, "fdcc")?;

    //--- spawn the orbital satellite actors
    let region = odin_orbital::load_config("conus.ron")?;
    let orbital_sat_configs = vec![ 
        "noaa-21_viirs.ron", "noaa-20_viirs.ron", "snpp_viirs.ron", 
        "landsat-8_oli.ron", "landsat-9_oli.ron",
        "sentinel-2a_msi.ron", "sentinel-2b_msi.ron", "sentinel-2c_msi.ron", // those don't have hotspot data yet
    ];
    let orbital_sats = spawn_orbital_hotspot_actors( &mut actor_system, pre_server.to_actor_handle(), region, &orbital_sat_configs)?;

    //--- spawn the sentinel actor
    let sentinel_name = arc!("sentinel");
    let hsentinel = spawn_actor!( actor_system, &sentinel_name, SentinelActor::new( 
        LiveSentinelConnector::new( odin_sentinel::load_config( "sentinel.ron")?), 
        dataref_action!( 
            let hserver: ActorHandle<SpaServerMsg> = pre_server.to_actor_handle(),
            let sender_id: Arc<String> = sentinel_name.clone() =>
            |_store: &SentinelStore| {
                Ok( hserver.try_send_msg( DataAvailable::new::<SentinelStore>(sender_id) )? )
            }
        ), 
        data_action!( 
            let hserver: ActorHandle<SpaServerMsg> = pre_server.to_actor_handle() => 
            |update:SentinelUpdate| {
                let ws_msg = WsMsg::json( SentinelService::mod_path(), "update", update)?;
                Ok( hserver.try_send_msg( BroadcastWsMsg{ws_msg})? )
            }
        )
    ))?;

    //--- finally spawn the server actor
    let _hserver = spawn_pre_actor!( actor_system, pre_server, SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "live",
        SpaServiceList::new()
            .add( build_service!( let hstore = hstore.clone() => ShareService::new( "odin_share_schema.js", hstore)))
            .add( build_service!( => GeoLayerService::new( &odin_geolayer::default_data_dir())))
            .add( build_service!( let hsentinel = hsentinel.clone() => SentinelService::new( hsentinel)))
            .add( build_service!( => GoesrHotspotService::new( goesr_sats)) )
            .add( build_service!( => OrbitalHotspotService::new( orbital_sats) ))
    ))?;

    Ok(())
});
