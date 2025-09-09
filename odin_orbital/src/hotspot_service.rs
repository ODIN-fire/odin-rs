/*
 * Copyright © 2025, United States Government, as represented by the Administrator of 
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

use std::{net::SocketAddr,fs,sync::Arc};
use std::any::type_name;
use async_trait::async_trait;

use axum::{
    http::{Uri,StatusCode},
    body::Body,
    routing::{Router,get},
    extract::{Path as AxumPath},
    response::{Response,IntoResponse},
};

use odin_actor::prelude::*;
use odin_common::json_writer::JsonWriter;
use odin_server::prelude::*;
use odin_cesium::ImgLayerService;

use crate::{ 
    PKG_CACHE_DIR, OrbitalSatelliteInfo, load_asset, 
    actor::{HotspotActorData, OrbitalHotspotActorMsg, ExecSnapshotAction},
    errors::Result
};

pub struct HotspotSat {
    pub sat_info: Arc<OrbitalSatelliteInfo>,
    pub hupdater: ActorHandle<OrbitalHotspotActorMsg>
}

impl HotspotSat {
    pub fn new (sat_info: Arc<OrbitalSatelliteInfo>, hupdater: ActorHandle<OrbitalHotspotActorMsg>)->Self {
        HotspotSat { sat_info, hupdater }
    }
}

pub struct OrbitalHotspotService {
    satellites: Vec<HotspotSat>,
}

impl OrbitalHotspotService {
    pub fn new (satellites: Vec<HotspotSat>)->Self { OrbitalHotspotService { satellites } }

    async fn data_handler (path: AxumPath<String>) -> Response {
        let pathname = PKG_CACHE_DIR.join( path.as_str());
        if pathname.is_file() {
            (StatusCode::OK, fs::read(pathname).unwrap()).into_response()
        } else {
            (StatusCode::NOT_FOUND, "sat data not found").into_response()
        }
    }

    fn sat_infos (&self)->Vec<Arc<OrbitalSatelliteInfo>> {
        self.satellites.iter().map(|s| s.sat_info.clone()).collect()
    }
}

#[async_trait]
impl SpaService for OrbitalHotspotService {

    fn add_dependencies (&self, spa_builder: SpaServiceList) -> SpaServiceList {
        spa_builder.add( build_service!( => ImgLayerService::new()))
    }

    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()>  {
        spa.add_assets( self_crate!(), load_asset);

        spa.add_module( asset_uri!( "odin_orbital_config.js"));
        spa.add_module( asset_uri!( "odin_orbital.js" ));

        spa.add_route( |router, spa_server_state| {
            router.route( &format!("/{}/orbital-data/{{*unmatched}}", spa_server_state.name.as_str()), get(Self::data_handler))
        });

        Ok(())
    }

    async fn data_available (&mut self, hself: &ActorHandle<SpaServerMsg>, has_connections: bool, sender_id: &str, data_type: &str) -> OdinServerResult<bool> {
        let mut is_our_data = false;

        if let Some(hupdater) = self.satellites.iter().find( |s| *s.hupdater.id == sender_id).map( |s| &s.hupdater) {
            if data_type == type_name::<HotspotActorData>() {
                if has_connections { // broadcast overpasses and hotspots to all current connections
                    let action = dyn_dataref_action!( 
                        let hself: ActorHandle<SpaServerMsg> = hself.clone() => 
                        |data: &HotspotActorData| {
                            send_hs_data( hself, None, data).await
                        }
                    );
                    hupdater.send_msg( ExecSnapshotAction(action)).await?;
                }
                is_our_data = true;
            }
        }

        Ok(is_our_data)
    }

    async fn init_connection (&mut self, hself: &ActorHandle<SpaServerMsg>, is_data_available: bool, conn: &mut WsConnection) -> OdinServerResult<()> {
        let remote_addr = conn.remote_addr;

        // no matter if we already have data we send our list of satellites (once)
        send_sat_infos( &hself, Some(remote_addr.clone()), &self.sat_infos()).await?;

        if is_data_available {
            for sat in &self.satellites {
                let action = dyn_dataref_action!{ 
                    let hself: ActorHandle<SpaServerMsg> = hself.clone(), 
                    let remote_addr: SocketAddr = remote_addr => 
                    |data: &HotspotActorData| { 
                        send_hs_data( hself, Some(*remote_addr), data).await
                    }
                };
                sat.hupdater.send_msg( ExecSnapshotAction(action)).await?;
            }
        }

        Ok(())
    }
}

async fn send_sat_infos (hself: &ActorHandle<SpaServerMsg>, remote_addr: Option<SocketAddr>, sat_infos: &Vec<Arc<OrbitalSatelliteInfo>>) -> OdinActorResult<()> {
    let mut w = JsonWriter::with_capacity( sat_infos.len() * 64);
    w.write_array(|w| {
        for si in sat_infos { si.write_basic_json_to( w) } 
    });
    let ws_msg = ws_msg_from_json( OrbitalHotspotService::mod_path(), "satellites", &w.to_string());

    if let Some(remote_addr) = remote_addr { // send to requesting connection
        hself.send_msg( SendWsMsg{remote_addr,ws_msg}).await?;
    } else {
        hself.send_msg( BroadcastWsMsg{ws_msg}).await?;
    }

    Ok(())
}

async fn send_hs_data (hself: &ActorHandle<SpaServerMsg>, remote_addr: Option<SocketAddr>, data: &HotspotActorData) -> std::result::Result<(),odin_action::OdinActionFailure> {
    let overpasses = ws_msg_from_json( OrbitalHotspotService::mod_path(), "overpasses", &data.serialize_collapsed_overpasses());
    let hotspots = ws_msg_from_json( OrbitalHotspotService::mod_path(), "hotspots", &data.serialize_collapsed_hotspots());

    if let Some(remote_addr) = remote_addr { // send to requesting connection
        hself.send_msg( SendWsMsg{remote_addr,ws_msg: overpasses}).await?;
        hself.send_msg( SendWsMsg{remote_addr,ws_msg: hotspots}).await?;

    } else { // broadcast to all connections
        hself.send_msg( BroadcastWsMsg{ws_msg: overpasses}).await?;
        hself.send_msg( BroadcastWsMsg{ws_msg: hotspots}).await?;
    }

    Ok(())
}