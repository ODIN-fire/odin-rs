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
#![allow(unused)]

use std::sync::Arc;

use odin_build;
use odin_common::{arc,json_writer::JsonWriter, vec_boxed};
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_himawari::{ self,
    HimawariConfig, HimawariHotspotStore, HimawariHotspotSet,
    service::HimawariHotspotService, actor::HimawariHotspotActor, live_importer::LiveHimawariHotspotImporter
};
use odin_orbital::{actor::spawn_orbital_hotspot_actors,hotspot_service::OrbitalHotspotService,firms::FirmsConfig};
use odin_share::prelude::*;
use odin_geolayer::GeoLayerService;
use odin_openmeteo::{self,OpenMeteoActor,OpenMeteoService};
use odin_wx::{WxServiceList,WxFileAvailable};
use odin_wind::{self, WindActor,WindActorMsg, server_subscribe_action, server_update_action, WindService};
use odin_bushfire::{Bushfire, BushfireService, BushfireStore, actor::{BushfireActor,BushfireActorMsg}, load_config, get_json_update_msg};


run_actor_system!( actor_system => {
    let pre_server = PreActorHandle::new( &actor_system, "server", 64);
    let pre_wx = PreActorHandle::new( &actor_system, "wx", 8);
    let pre_fire = PreActorHandle::new( &actor_system, "bushfires", 8);

    let svc_list = SpaServiceList::new();

    //--- spawn the shared item store actor (needed by WindService)
    let hstore = spawn_server_share_actor(&mut actor_system, "share", pre_server.to_actor_handle(), default_shared_items(), false)?;
    let svc_list = svc_list.add( build_service!( let hstore = hstore.clone() => ShareService::new( "odin_share_schema.js", hstore)));

    //--- add the geolayer service
    let svc_list = svc_list.add( build_service!( => GeoLayerService::new( &odin_geolayer::default_data_dir())));

    //--- bushfire actor
    let fire_id = pre_fire.get_id();
    let hfire = spawn_pre_actor!( actor_system, pre_fire,
        BushfireActor::new(
            load_config("bushfire.ron")?,
            dataref_action!(
                let sender_id: Arc<String> = fire_id,
                let hserver: ActorHandle<SpaServerMsg> = pre_server.to_actor_handle() => |store: &BushfireStore| {
                    Ok( hserver.try_send_msg( DataAvailable::new::<BushfireStore>(sender_id) )? )
                }
            ),
            data_action!(
                let hserver: ActorHandle<SpaServerMsg> = pre_server.to_actor_handle() => |updates: Vec<Bushfire>| {
                    let ws_msg = get_json_update_msg( &updates);
                    Ok( hserver.try_send_msg( BroadcastWsMsg{ws_msg})? )
                }
            )
        )
    )?;
    let svc_list = svc_list.add( build_service!( => BushfireService::new( hfire) ));

    //--- wind actors
    let wxs: WxServiceList = vec_boxed![ OpenMeteoService::new_basic_ifs( pre_wx.to_actor_handle()) ];

    let hwind = spawn_actor!( actor_system, "wind",
        WindActor::new(
            odin_wind::load_config("wind.ron")?,
            wxs,
            server_subscribe_action( pre_server.to_actor_handle()),
            server_update_action( pre_server.to_actor_handle())
        ), 64
    )?;
    let svc_list = svc_list.add( build_service!( let hwind = hwind.clone() => WindService::new( hwind) ));

    let hwx = spawn_pre_actor!( actor_system, pre_wx, OpenMeteoActor::new(
        odin_openmeteo::load_config( "openmeteo.ron")?,
        data_action!( let hwind: ActorHandle<WindActorMsg> = hwind.clone() => |data: WxFileAvailable| {
            Ok( hwind.try_send_msg( data)? )
        })
    ))?;

    //--- spawn Himawari actor
    let config: Arc<HimawariConfig> = Arc::new( odin_himawari::load_config("himawari.ron")?);
    let himawari = spawn_actor!( actor_system, "himawari", HimawariHotspotActor::new(
        config.clone(),
        LiveHimawariHotspotImporter::new( config, Arc::new( odin_himawari::PKG_CACHE_DIR.clone())),
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
    let svc_list = svc_list.add( build_service!( => HimawariHotspotService::new( himawari)));

    //--- spawn the orbital satellite actors
    let region = odin_orbital::load_region_config( "australia.ron")?;
    let data = odin_orbital::load_config( "firms-aus.ron")?;
    let orbital_sat_configs = vec![
        "noaa-21_viirs.ron", "noaa-20_viirs.ron", "snpp_viirs.ron",
        "sentinel-2a_msi.ron", "sentinel-2b_msi.ron", "sentinel-2c_msi.ron", // those don't have hotspot data yet
    ];
    let orbital_sats = spawn_orbital_hotspot_actors( &mut actor_system, pre_server.to_actor_handle(), region, data, &orbital_sat_configs)?;
    let svc_list = svc_list.add( build_service!( => OrbitalHotspotService::new( orbital_sats)));

    //--- finally spawn the server actor
    let _hserver = spawn_pre_actor!( actor_system, pre_server, SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "live_aus",
        svc_list
    ))?;

    Ok(())
});
