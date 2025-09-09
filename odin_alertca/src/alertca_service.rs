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

use std::{net::SocketAddr,any::type_name,fs, time::Duration};
use axum::{
    http::{Uri,StatusCode},
    body::Body,
    routing::{Router,get},
    extract::{Path as AxumPath},
    response::{Response,IntoResponse},
};
use async_trait::async_trait;

use odin_actor::prelude::*;
use odin_build::pkg_cache_dir;
use odin_server::prelude::*;
use odin_cesium::ImgLayerService;

use crate::{load_asset, CameraStore, actor::{ExecSnapshotAction,AlertCaActorMsg}};

pub struct AlertCaService {
    hactor: ActorHandle<AlertCaActorMsg>
}

impl AlertCaService {
    pub fn new (hactor: ActorHandle<AlertCaActorMsg>)->Self {
        AlertCaService{hactor}
    }

    async fn data_handler (path: AxumPath<String>) -> impl IntoResponse {
        // this is served from our cache dir 
        let server_path = pkg_cache_dir!().join( path.as_str());
        odin_server::file_response( &server_path, true).await.into_response()
    }
}

#[async_trait]
impl SpaService for AlertCaService {

    fn add_dependencies (&self, spa_builder: SpaServiceList) -> SpaServiceList {
        spa_builder.add( build_service!( => ImgLayerService::new()))
    }

    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()>  {
        spa.add_assets( self_crate!(), load_asset);

        spa.add_module( asset_uri!( "odin_alertca_config.js"));
        spa.add_module( asset_uri!( "odin_alertca.js" ));

        spa.add_route( |router, spa_server_state| { // serve image files
            router.route( &format!("/{}/alertca-data/{{*unmatched}}", spa_server_state.name.as_str()), get(Self::data_handler))
        });

        Ok(())
    }

    async fn data_available (&mut self, hself: &ActorHandle<SpaServerMsg>, has_connections: bool, sender_id: &str, data_type: &str) -> OdinServerResult<bool> {
        let mut is_our_data = false;

        if self.hactor.id() == sender_id && data_type == type_name::<CameraStore>() { // is this for us?
            if has_connections {
                let action = dyn_dataref_action!( let hself: ActorHandle<SpaServerMsg> = hself.clone() => |store: &CameraStore| {
                    let ws_msg = store.get_json_snapshot_msg();
                    Ok( hself.try_send_msg( BroadcastWsMsg{ws_msg})? )
                });
                self.hactor.send_msg( ExecSnapshotAction(action)).await?;
            }
            is_our_data = true;
        }
        Ok(is_our_data) // either not for us or we don't have connections yet
    }


    async fn init_connection (&mut self, hself: &ActorHandle<SpaServerMsg>, is_data_available: bool, conn: &mut WsConnection) -> OdinServerResult<()> {
        let remote_addr = conn.remote_addr;

        if is_data_available {
            let action = dyn_dataref_action!{
                let hself: ActorHandle<SpaServerMsg> = hself.clone(),  // this is the server handle
                let remote_addr: SocketAddr = remote_addr => 
                |store: &CameraStore| {                              // this gets executed by the AlertCaActor 
                    let remote_addr = remote_addr.clone();
                    let ws_msg = store.get_json_snapshot_msg();
                    Ok( hself.try_send_msg( SendWsMsg{remote_addr,ws_msg})? )
                }
            };
            self.hactor.send_msg( ExecSnapshotAction(action)).await?; // send the action requests to the AlertCaActor
        }
        
        Ok(())
    }
}