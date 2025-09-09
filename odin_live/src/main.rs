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
use odin_common::{arc,json_writer::JsonWriter};
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_goesr::{GoesrHotspotService, actor::spawn_goesr_hotspot_actors};
use odin_orbital::{actor::spawn_orbital_hotspot_actors,hotspot_service::OrbitalHotspotService};
use odin_share::prelude::*;
use odin_geolayer::GeoLayerService;
use odin_sentinel::{SentinelStore, SentinelUpdate, LiveSentinelConnector, SentinelActor, sentinel_service::SentinelService};
use odin_hrrr::{self,HrrrActor,HrrrFileAvailable};
use odin_wind::{self, actor::{WindActor,WindActorMsg, server_subscribe_action, server_update_action}, wind_service::WindService};
use odin_adsb::{AircraftStore,actor::AdsbActor,adsb_service::AdsbService, sbs::SbsConnector};
use odin_n5::{self, N5DeviceStore, N5DataUpdate, n5_service::N5Service, actor::N5Actor, live_connector::LiveN5Connector};
use odin_alertca::{self,actor::AlertCaActor, alertca_service::AlertCaService, live_connector::LiveAlertCaConnector, CameraStore, CameraUpdate};

run_actor_system!( actor_system => {
    let pre_server = PreActorHandle::new( &actor_system, "server", 64);
    let pre_hrrr = PreActorHandle::new( &actor_system, "hrrr", 8);
    let pre_n5 = PreActorHandle::new( &actor_system, "n5", 8);
    let pre_aca = PreActorHandle::new( &actor_system, "alertca", 8);

    //--- spawn the shared item store actor (needed by WindService)
    let hstore = spawn_server_share_actor(&mut actor_system, "share", pre_server.to_actor_handle(), default_shared_items(), false)?;

    //--- spawn the micro grid wind simulator
    let hwind = spawn_actor!( actor_system, "wind", WindActor::new(
        odin_wind::load_config("wind.ron")?,
        pre_hrrr.to_actor_handle(),
        server_subscribe_action( pre_server.to_actor_handle()),
        server_update_action( pre_server.to_actor_handle()) 
    ))?;

    //--- spawn the HRRR weather forecast importer
    let _hrrr = spawn_pre_actor!( actor_system, pre_hrrr, HrrrActor::with_statistic_schedules(
        odin_hrrr::load_config( "hrrr_conus-8.ron")?,
        data_action!( let hwind: ActorHandle<WindActorMsg> = hwind.clone() => |data: HrrrFileAvailable| {
            Ok( hwind.try_send_msg( data)? )
        })
    ).await? )?;

    //--- spawn the GOES-R actors
    let goesr_sat_configs = vec![ "goes_18.ron", "goes_19.ron" ];
    let goesr_sats = spawn_goesr_hotspot_actors( &mut actor_system, pre_server.to_actor_handle(), &goesr_sat_configs, "fdcc")?;

    //--- spawn the orbital satellite actors
    let region = odin_orbital::load_config("conus.ron")?;
    let orbital_sat_configs = vec![ 
        "noaa-21_viirs.ron", "noaa-20_viirs.ron", "snpp_viirs.ron", 
        "landsat-8_oli.ron", "landsat-9_oli.ron",
        "sentinel-2a_msi.ron", "sentinel-2b_msi.ron", "sentinel-2c_msi.ron", // those don't have hotspot data yet
        "otc-1.ron", "otc-2.ron", "otc-3.ron", "otc-4.ron", "otc-5.ron", "otc-6.ron", "otc-7.ron", "otc-8.ron",
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

    //--- spawn N5Actor
    let sender_id = pre_n5.get_id();
    let hn5 = spawn_pre_actor!( actor_system, pre_n5, 
        N5Actor::new( 
            LiveN5Connector::new( odin_n5::load_config("n5.ron")?),
            dataref_action!( 
                let sender_id: Arc<String> = sender_id,
                let hserver: ActorHandle<SpaServerMsg> = pre_server.to_actor_handle() => |_store: &N5DeviceStore| {
                    Ok( hserver.try_send_msg( DataAvailable::new::<N5DeviceStore>(sender_id) )? )
                }
            ),
            data_action!( 
                let hserver: ActorHandle<SpaServerMsg> = pre_server.to_actor_handle() => |updates: Vec<N5DataUpdate>| {
                    let ws_msg = odin_n5::get_json_update_msg( &updates);
                    Ok( hserver.try_send_msg( BroadcastWsMsg{ws_msg})? )
                }
            )
        )
    )?;

    //--- spawn AlertCaActor
    let sender_id = pre_aca.get_id();
    let haca = spawn_pre_actor!( actor_system, pre_aca,
        AlertCaActor::new( 
            odin_alertca::load_config("sf_bay_area.ron")?,
            LiveAlertCaConnector::new,
            dataref_action!( 
                let sender_id: Arc<String> = sender_id, 
                let hserver: ActorHandle<SpaServerMsg> = pre_server.to_actor_handle() => |_store: &CameraStore| {
                    Ok( hserver.try_send_msg( DataAvailable::new::<CameraStore>(sender_id) )? )
                }
            ),
            dataref_action!( 
                let hserver: ActorHandle<SpaServerMsg> = pre_server.to_actor_handle() => |updates: &Vec<CameraUpdate>| {
                    let ws_msg = odin_alertca::get_json_update_msg( &updates);
                    Ok( hserver.try_send_msg( BroadcastWsMsg{ws_msg})? )
                }
            )
        )
    )?;

    //--- spawn the AdsbActor
    let hadsb = spawn_actor!( actor_system, "adsb",
        AdsbActor::<SbsConnector,_>::new(
            odin_adsb::load_config("adsb.ron")?, 
            dataref_mut_action!(  
                let mut w: JsonWriter = JsonWriter::with_capacity(4096), // use a cached writer to assemble the ws_msg
                let mut hserver: ActorHandle<SpaServerMsg> = pre_server.to_actor_handle() => 
                |store: &AircraftStore| {
                    let ws_msg = store.get_json_update_msg(w);
                    Ok( hserver.try_send_msg( BroadcastWsMsg{ws_msg})? )
                }
            )
        )
    )?;

    //--- finally spawn the server actor
    let _hserver = spawn_pre_actor!( actor_system, pre_server, SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "live",
        SpaServiceList::new()
            .add( build_service!( let hstore = hstore.clone() => ShareService::new( "odin_share_schema.js", hstore)))
            .add( build_service!( => GeoLayerService::new( &odin_geolayer::default_data_dir())))
            .add( build_service!( => WindService::new( hwind) ))
            .add( build_service!( => SentinelService::new( hsentinel)))
            .add( build_service!( => N5Service::new( hn5) ))
            .add( build_service!( => AlertCaService::new( haca) ))
            .add( build_service!( => GoesrHotspotService::new( goesr_sats)) )
            .add( build_service!( => OrbitalHotspotService::new( orbital_sats) ))
            .add( build_service!( => AdsbService::new( vec![hadsb]) ))
    ))?;

    Ok(())
});
