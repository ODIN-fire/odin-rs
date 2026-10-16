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

use std::sync::Arc;

use odin_build;
use odin_common::{arc,json_writer::JsonWriter, vec_boxed};
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_goesr::{GoesrHotspotService, actor::spawn_goesr_hotspot_actors};
use odin_orbital::{actor::spawn_orbital_hotspot_actors,hotspot_service::OrbitalHotspotService,firms::FirmsConfig};
use odin_share::prelude::*;
use odin_geolayer::GeoLayerService;
use odin_sentinel::{LiveSentinelConnector, SentinelActor, sentinel_service::SentinelService};
use odin_hrrr::{self,HrrrActor,HrrrService};
use odin_openmeteo::{self,OpenMeteoActor,wx_service::OpenMeteoService};
use odin_wx::{WxServiceList,WxFileAvailable};
use odin_wind::{self, WindActor,WindActorMsg, server_subscribe_action, server_update_action, WindService};
use odin_adsb::{actor::AdsbActor,adsb_service::AdsbService, sbs::SbsConnector};
use odin_n5::{self, n5_service::N5Service, actor::N5Actor, live_connector::LiveN5Connector};
use odin_alertca::{self,actor::AlertCaActor, alertca_service::AlertCaService, live_connector::LiveAlertCaConnector, CameraStore, CameraUpdate};
use odin_fires::{fire_service::FireService};
use odin_fems::{self, FemsStore, FemsStation, actor::FemsActor,service::FemsService};
use odin_landfire::{service::LandfireService,LandfireConfig};
use odin_elmfire::{self,ElmfireRun, ElmfireService, actor::{ElmfireActor,ElmfireActorMsg}};
use odin_nfdrs::{self, NfdrsActor, NfdrsService};
use odin_openmeteo::ids::ModelId;


// note that odin_sentinel, odin_n5 and odin_adsb all require non-public data sources and hence are feature gated

run_actor_system!( actor_system => {
    let pre_server = PreActorHandle::new( &actor_system, "server", 64);
    let pre_hrrr = PreActorHandle::new( &actor_system, "hrrr", 8);

    let svc_list = SpaServiceList::new();

    //--- spawn the shared item store actor (needed by WindService)
    let hstore = spawn_server_share_actor(&mut actor_system, "share", pre_server.to_actor_handle(), default_shared_items(), false)?;
    let svc_list = svc_list.add( build_service!( let hstore = hstore.clone() => ShareService::new( "odin_share_schema.js", hstore)));

    //--- add the geolayer service
    let svc_list = svc_list.add( build_service!( => GeoLayerService::new( &odin_geolayer::default_data_dir())));

    //--- add the historical fires service
    let svc_list = svc_list.add( build_service!(let svc = FireService::new(odin_fires::load_config("fires.ron")?)? => svc));

    //--- add the landfire service
    let svc_list = svc_list.add( build_service!( let svc = LandfireService::new(odin_landfire::load_config("landfire.ron")?) => svc));

    //--- spawn the NFDRS actor (fuel moisture and fire danger index grids)
    let hnfdrs = spawn_actor!( actor_system, "nfdrs",
        NfdrsActor::new(
            odin_nfdrs::load_config("nfdrs.ron")?,
            odin_openmeteo::load_config("openmeteo.ron")?,
            ModelId::ncep_hrrr_conus,                          // determines NFDRS grid resolution
            odin_landfire::load_config("lf_2025_conus.ron")?,  // GeoDs for slope class and fuel model of grid points
            odin_nfdrs::actor::server_update_action( pre_server.to_actor_handle()),
            odin_nfdrs::actor::server_remove_action( pre_server.to_actor_handle())
        )
    )?;
    let svc_list = svc_list.add( build_service!( let svc = NfdrsService::new( hnfdrs) => svc));

    //--- spawn FemsActor
    let fems_id = "fems";
    let hfems = spawn_actor!( actor_system, fems_id,
        FemsActor::new(
            odin_fems::load_config("fems.ron")?,
            odin_fems::actor::server_init_action( pre_server.to_actor_handle(), fems_id),
            odin_fems::actor::server_update_action( pre_server.to_actor_handle())
        )
    )?;
    let svc_list = svc_list.add( build_service!( => FemsService::new(  hfems)));

    //--- spawn the micro grid wind simulator
    let wxs: WxServiceList = vec_boxed![ HrrrService::new_basic( pre_hrrr.to_actor_handle()) ];
    let hwind = spawn_actor!( actor_system, "wind", WindActor::new(
        odin_wind::load_config("wind.ron")?,
        wxs,
        odin_wind::actor::server_subscribe_action( pre_server.to_actor_handle()),
        odin_wind::actor::server_update_action( pre_server.to_actor_handle())
    ))?;
    let svc_list = svc_list.add( build_service!( let hwind = hwind.clone() => WindService::new( hwind) ));

    //--- spawn the elmfire actor
    let hfire = spawn_actor!( actor_system, "elmfire",
        ElmfireActor::new(
            odin_elmfire::load_config("elmfire.ron")?,
            odin_elmfire::actor::server_update_action( pre_server.to_actor_handle()),
            odin_elmfire::actor::server_remove_action( pre_server.to_actor_handle())
        )
    )?;
    let svc_list = svc_list.add( build_service!( let svc = ElmfireService::new( hfire) => svc));

    //--- spawn the HRRR weather forecast importer
    let _hrrr = spawn_pre_actor!( actor_system, pre_hrrr, HrrrActor::with_statistic_schedules(
        odin_hrrr::load_config( "hrrr_conus-8.ron")?,
        data_action!( let hwind: ActorHandle<WindActorMsg> = hwind.clone() => |data: WxFileAvailable| {
            Ok( hwind.try_send_msg( data)? )
        })
    ).await? )?;

    //--- spawn the GOES-R actors
    let goesr_sat_configs = vec![ "goes_18.ron", "goes_19.ron" ];
    let goesr_sats = spawn_goesr_hotspot_actors( &mut actor_system, pre_server.to_actor_handle(), &goesr_sat_configs, "fdcc")?;
    let svc_list = svc_list.add( build_service!( => GoesrHotspotService::new( goesr_sats)));

    //--- spawn the orbital satellite actors
    let region = odin_orbital::load_region_config( "conus.ron")?;
    let data = odin_orbital::load_config( "firms.ron")?;
    let orbital_sat_configs = vec![
        "noaa-21_viirs.ron", "noaa-20_viirs.ron", "snpp_viirs.ron",
        "landsat-8_oli.ron", "landsat-9_oli.ron",
        "sentinel-2a_msi.ron", "sentinel-2b_msi.ron", "sentinel-2c_msi.ron", // those don't have hotspot data yet
        //"otc-1.ron", "otc-2.ron", "otc-3.ron", "otc-4.ron", "otc-5.ron", "otc-6.ron", "otc-7.ron", "otc-8.ron",
    ];
    let orbital_sats = spawn_orbital_hotspot_actors( &mut actor_system, pre_server.to_actor_handle(), region, data, &orbital_sat_configs)?;
    let svc_list = svc_list.add( build_service!( => OrbitalHotspotService::new( orbital_sats)));

    //--- spawn AlertCaActor
    let sender_id = "alertca";
    let haca = spawn_actor!( actor_system, sender_id, AlertCaActor::new(
        odin_alertca::load_config("sf_bay_area.ron")?,
        LiveAlertCaConnector::new,
        odin_alertca::actor::server_init_action( pre_server.to_actor_handle(), sender_id),
        odin_alertca::actor::server_update_action( pre_server.to_actor_handle())
    ))?;
    let svc_list = svc_list.add( build_service!( => AlertCaService::new(  haca)));

    //--- spawn the sentinel actor
    #[cfg(feature="sentinel")]
    let svc_list = {
        let sentinel_id = "sentinel";
        let hsentinel = spawn_actor!( actor_system, sentinel_id, SentinelActor::new(
            LiveSentinelConnector::new( odin_sentinel::load_config( "sentinel.ron")?),
            odin_sentinel::actor::server_init_action( pre_server.to_actor_handle(), sentinel_id),
            odin_sentinel::actor::server_update_action( pre_server.to_actor_handle())
        ))?;
        svc_list.add( build_service!( => SentinelService::new( hsentinel)))
    };

    //--- spawn N5Actor
    #[cfg(feature="n5")]
    let svc_list = {
        let n5_id = "n5";
        let hn5 = spawn_actor!( actor_system, n5_id, N5Actor::new(
            LiveN5Connector::new( odin_n5::load_config("n5.ron")?),
            odin_n5::actor::server_init_action( pre_server.to_actor_handle(), n5_id),
            odin_n5::actor::server_update_action( pre_server.to_actor_handle())
        ))?;
        svc_list.add( build_service!( => N5Service::new( hn5)))
    };

    //--- spawn the AdsbActor
    #[cfg(feature="adsb")]
    let svc_list = {
        let hadsb = spawn_actor!( actor_system, "adsb", AdsbActor::<SbsConnector,_>::new(
            odin_adsb::load_config("adsb.ron")?,
            odin_adsb::actor::server_update_action( pre_server.to_actor_handle())
        ))?;
        svc_list.add( build_service!( => AdsbService::new( vec![ hadsb])))
    };

    //--- finally spawn the server actor
    let _hserver = spawn_pre_actor!( actor_system, pre_server, SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "live",
        svc_list
    ))?;

    Ok(())
});
