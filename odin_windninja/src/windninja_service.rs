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

pub struct WindNinjaService {
    hwind: ActorHandle<WindNinjaActorMsg>,
    // TODO
}

impl SpaService for WindNinjaService {
    fn add_dependencies (&self, spa_builder: SpaServiceList) -> SpaServiceList {
        spa_builder
            .add( build_service!( => UiService::new()))
            .add( build_service!( => WsService::new()))
            .add( build_service!( => CesiumService::new()))
    }

    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()>  {
        spa.add_assets( self_crate!(), load_asset);

        spa.add_module( asset_uri!( "odin_windninja_config.js"));
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
                            //send_hs_data( hself, None, data).await
                        }
                    );
                    hupdater.send_msg( ExecSnapshotAction(action)).await?;
                }
                is_our_data = true;
            }
        }

        Ok(is_our_data)
    }

    async fn init_connection (&mut self, hself: &ActorHandle<SpaServerMsg>, is_data_available: bool, conn: &mut SpaConnection) -> OdinServerResult<()> {
        let remote_addr = conn.remote_addr;

        // no matter if we already have data we send our list of satellites (once)
        send_sat_infos( &hself, Some(remote_addr.clone()), &self.sat_infos()).await?;

        if is_data_available {
            for sat in &self.satellites {
                let action = dyn_dataref_action!{ 
                    let hself: ActorHandle<SpaServerMsg> = hself.clone(), 
                    let remote_addr: SocketAddr = remote_addr => 
                    |data: &HotspotActorData| { 
                        //send_hs_data( hself, Some(*remote_addr), data).await
                    }
                };
                sat.hupdater.send_msg( ExecSnapshotAction(action)).await?;
            }
        }

        Ok(())
    }

        /// this is how we get data from clients. Called from ws input task of respective connection
    async fn handle_ws_msg (&mut self, 
        hself: &ActorHandle<SpaServerMsg>, remote_addr: &SocketAddr, ws_msg_parts: &WsMsgParts) -> OdinServerResult<WsMsgReaction> 
    {
        if ws_msg_parts.mod_path == ShareService::mod_path() {
            match ws_msg_parts.msg_type {
            }
        }
    }
}