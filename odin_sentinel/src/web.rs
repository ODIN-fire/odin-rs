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

use odin_build::prelude::*;
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_cesium::ImgLayerService;

use crate::{
    load_config, load_asset, sentinel_cache_dir, ExecSnapshotAction, SentinelActorMsg, SentinelStore, SentinelDeviceInfo, SentinelDeviceInfos
};

pub const JS_MOD_PATH: &'static str = "odin_sentinel/odin_sentinel.js";

/// SpaService to show sentinel infos on a cesium display
pub struct SentinelService {
    device_infos: SentinelDeviceInfos,
    hsentinel: ActorHandle<SentinelActorMsg>, // our data source
}

impl SentinelService {
    pub fn new (hsentinel: ActorHandle<SentinelActorMsg>, )->Self { 
        let device_infos = load_config("sentinel_info.ron").expect("failed to load sentinel_info.ron config"); // Ok to panic in ctor
        SentinelService{device_infos,hsentinel}
    }

    async fn image_handler (path: AxumPath<String>) -> Response {
        let pathname = sentinel_cache_dir().join( path.as_str());
        if pathname.is_file() {
            (StatusCode::OK, fs::read(pathname).unwrap()).into_response()
        } else {
            (StatusCode::NOT_FOUND, "image not found").into_response() // FIXME - it might be in flight so we should wait for the download to complete
        }
    }
}

#[async_trait]
impl SpaService for SentinelService {
    fn add_dependencies (&self, spa_builder: SpaServiceList) -> SpaServiceList {
        spa_builder.add( build_service!( ImgLayerService::new()))
    }

    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()>  {
        spa.add_assets( self_crate!(), load_asset);
        spa.add_module( asset_uri!("odin_sentinel_config.js"));
        spa.add_module( asset_uri!("odin_sentinel.js"));

        spa.add_route( |router, spa_server_state| {
            router.route( &format!("/{}/sentinel-image/*unmatched", spa_server_state.name.as_str()), get(Self::image_handler))
        });

        Ok(())
    }

    async fn data_available (&mut self, hself: &ActorHandle<SpaServerMsg>, has_connections: bool, sender_id: &str, data_type: &str) -> OdinServerResult<bool> {
        let mut is_our_data = false;

        if self.hsentinel.id() == sender_id && data_type == type_name::<SentinelStore>() { // is this for us?
            if has_connections {
                let action = dyn_dataref_action!( hself.clone(): ActorHandle<SpaServerMsg> => |data: &SentinelStore| {
                    let sentinels = data.values();
                    let data = ws_msg!( JS_MOD_PATH, sentinels).to_json()?;
                    Ok( hself.try_send_msg( BroadcastWsMsg{data})? )
                });
                self.hsentinel.send_msg( ExecSnapshotAction(action)).await?;
            }
            is_our_data = true;
        }
        Ok(is_our_data) // either not for us or we don't have connections yet
    }

    // send an ExecSnapshotAction to the SentinelActor to send a JSON websocket message to the new connection
    async fn init_connection (&mut self, hself: &ActorHandle<SpaServerMsg>, is_data_available: bool, conn: &mut SpaConnection) -> OdinServerResult<()> {
        let remote_addr = conn.remote_addr;

        let device_infos = &self.device_infos;
        let data = ws_msg!( JS_MOD_PATH, device_infos).to_json()?;
        hself.try_send_msg( SendWsMsg{remote_addr,data})?;

        if is_data_available {
            let action = dyn_dataref_action!( hself.clone(): ActorHandle<SpaServerMsg>, remote_addr: SocketAddr => |data: &SentinelStore| {
                let sentinels = data.values();
                let data = ws_msg!( JS_MOD_PATH, sentinels).to_json()?;
                let remote_addr = remote_addr.clone();
                Ok( hself.try_send_msg( SendWsMsg{remote_addr,data})? )
            });
            self.hsentinel.send_msg( ExecSnapshotAction(action)).await?;
        }
        Ok(())
    }
}