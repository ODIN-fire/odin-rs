#![allow(unused)]

use std::sync::Arc;

use odin_build;
use odin_common::{arc,json_writer::JsonWriter};
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_himawari::{ self,
    HimawariConfig, HimawariHotspotStore, HimawariHotspotSet,
    service::HimawariHotspotService, actor::HimawariHotspotActor, live_importer::LiveHimawariHotspotImporter
};
use odin_orbital::{actor::spawn_orbital_hotspot_actors,hotspot_service::OrbitalHotspotService,firms::FirmsConfig};
use odin_share::prelude::*;
use odin_geolayer::GeoLayerService;

run_actor_system!( actor_system => {
    let pre_server = PreActorHandle::new( &actor_system, "server", 64);

    let svc_list = SpaServiceList::new();

    //--- spawn the shared item store actor (needed by WindService)
    let hstore = spawn_server_share_actor(&mut actor_system, "share", pre_server.to_actor_handle(), default_shared_items(), false)?;
    let svc_list = svc_list.add( build_service!( let hstore = hstore.clone() => ShareService::new( "odin_share_schema.js", hstore)));

    //--- add the geolayer service
    let svc_list = svc_list.add( build_service!( => GeoLayerService::new( &odin_geolayer::default_data_dir())));

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
