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

use std::{net::SocketAddr,any::type_name,fs};
use async_trait::async_trait;
use axum::{
    http::{Uri,StatusCode},
    body::Body,
    routing::{Router,get},
    extract::{Path as AxumPath},
    response::{Response,IntoResponse},
};
use serde::{Serialize,Deserialize};

use odin_build::prelude::*;
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_cesium::ImgLayerService;

use crate::{load_asset, load_config, GoesrHotspotImportActorMsg, GoesrHotspotStore, ExecSnapshotAction};

//--- aux types for creating JSON messages

#[derive(Debug,Serialize,Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct GoesrSatelliteInfo {
    pub sat_id: u32,
    pub name: String,
    pub description: String,
    pub show: bool,
}

pub struct GoesrHotspotSat {
    pub info: GoesrSatelliteInfo,
    pub hupdater: ActorHandle<GoesrHotspotImportActorMsg>
}

impl GoesrHotspotSat {
    pub fn new( info: GoesrSatelliteInfo, hupdater: ActorHandle<GoesrHotspotImportActorMsg>)->Self { GoesrHotspotSat { info, hupdater } }
}

//--- the SpaService

/// microservice for GOES-R hotspot data
pub struct GoesrHotspotService {
    satellites: Vec<GoesrHotspotSat>,
}

impl GoesrHotspotService {
    pub fn new (satellites: Vec<GoesrHotspotSat>)-> Self { GoesrHotspotService{satellites} }
}

#[async_trait]
impl SpaService for GoesrHotspotService {

    fn add_dependencies (&self, spa_builder: SpaServiceList) -> SpaServiceList {
        spa_builder.add( build_service!( => ImgLayerService::new()))
    }

    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()>  {
        spa.add_assets( self_crate!(), load_asset);
        spa.add_module( asset_uri!("odin_goesr_config.js"));
        spa.add_module( asset_uri!( "odin_goesr.js" ));

        Ok(())
    }

    async fn data_available (&mut self, hself: &ActorHandle<SpaServerMsg>, has_connections: bool, sender_id: &str, data_type: &str) -> OdinServerResult<bool> {
        let mut is_our_data = false;
        
        if let Some(hupdater) = self.satellites.iter().find( |s| *s.hupdater.id == sender_id).map( |s| &s.hupdater) {
            if data_type == type_name::<GoesrHotspotStore>() {
                if has_connections {
                    let action = dyn_dataref_action!( let hself: ActorHandle<SpaServerMsg> = hself.clone() => |store: &GoesrHotspotStore| {
                        for hotspots in store.iter_old_to_new(){
                            let ws_msg = WsMsg::json( GoesrHotspotService::mod_path(), "hotspots", hotspots)?;
                            hself.try_send_msg( BroadcastWsMsg{ws_msg})?;
                        }
                        Ok(())
                    });
                    hupdater.send_msg( ExecSnapshotAction(action)).await?;
                }
                is_our_data = true;
            }
        }

        Ok(is_our_data)
    }

    async fn init_connection (&mut self, hself: &ActorHandle<SpaServerMsg>, is_data_available: bool, conn: &mut WsConnection) -> OdinServerResult<()> {
        let satellites: Vec<&GoesrSatelliteInfo> = self.satellites.iter().map( |s| &s.info).collect();
        let ws_msg = WsMsg::json( GoesrHotspotService::mod_path(), "satellites", satellites)?;
        conn.send( ws_msg).await;

        if is_data_available {
            let remote_addr = conn.remote_addr;
            for sat in &self.satellites {
                let action = dyn_dataref_action!{ 
                    let hself: ActorHandle<SpaServerMsg> = hself.clone(), 
                    let remote_addr: SocketAddr = remote_addr => 
                    |store: &GoesrHotspotStore| {
                        for hotspots in store.iter_old_to_new(){
                            let remote_addr = remote_addr.clone();
                            let ws_msg = WsMsg::json( GoesrHotspotService::mod_path(), "hotspots", hotspots)?;
                            hself.try_send_msg( SendWsMsg{remote_addr,ws_msg})?;
                        }
                        Ok(())
                    }
                };
                sat.hupdater.send_msg( ExecSnapshotAction(action)).await?;
            }
        }

        Ok(())
    }
}
