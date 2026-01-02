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

use std::{net::SocketAddr,any::type_name,fs};
use async_trait::async_trait;
use serde::{Serialize,Deserialize};
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_cesium::ImgLayerService;

use crate::{load_asset, HimawariHotspotStore, actor::{HimawariHotspotActorMsg, ExecSnapshotAction}};

/// SpaService implementor for Himawari hotspots
pub struct HimawariHotspotService {
    hupdater: ActorHandle<HimawariHotspotActorMsg>
}

impl HimawariHotspotService {
    pub fn new (hupdater: ActorHandle<HimawariHotspotActorMsg>)->Self {
        HimawariHotspotService{ hupdater }
    }
}

#[async_trait]
impl SpaService for HimawariHotspotService {

    fn add_dependencies (&self, spa_builder: SpaServiceList) -> SpaServiceList {
        spa_builder.add( build_service!( => ImgLayerService::new())) // we need a Cesium virtual globe to display hotspots on
    }

    fn add_components (&self,spa: &mut SpaComponents) -> OdinServerResult<()>  {
        spa.add_assets( self_crate!(), load_asset);
        spa.add_module( asset_uri!("odin_himawari_config.js"));
        spa.add_module( asset_uri!( "odin_himawari.js" ));

        Ok(())
    }

    /// overload the default implementation that handles HimawariHotspotActor initialization notifications.
    /// if we have live connections at the time of this notification we have to send the data to all of them
    async fn data_available (&mut self, hself: &ActorHandle<SpaServerMsg>, has_connections: bool, sender_id: &str, data_type: &str) -> OdinServerResult<bool> {
        let mut is_our_data = false;
        if self.hupdater.id.as_str() == sender_id {
            if data_type == type_name::<HimawariHotspotStore>() {
                if has_connections {
                    let action = dyn_dataref_action!( let hself: ActorHandle<SpaServerMsg> = hself.clone() => |store: &HimawariHotspotStore| {
                        for hotspots in store.iter_old_to_new(){
                            let ws_msg = WsMsg::json( HimawariHotspotService::mod_path(), "hotspots", hotspots)?;
                            hself.try_send_msg( BroadcastWsMsg{ws_msg})?;
                        }
                        Ok(())
                    });
                    self.hupdater.send_msg( ExecSnapshotAction(action)).await?;
                }
                is_our_data = true;
            }
        }

        Ok(is_our_data)
    }

    /// overload default implementation that handles new connection requests, based on if the corresponding HimawariHotspotActor
    /// already has data (and sent us a notification)
    async fn init_connection (&mut self, hself: &ActorHandle<SpaServerMsg>, is_data_available: bool, conn: &mut WsConnection) -> OdinServerResult<()> {
        if is_data_available {
            let remote_addr = conn.remote_addr;
            let action = dyn_dataref_action!{
                let hself: ActorHandle<SpaServerMsg> = hself.clone(),
                let remote_addr: SocketAddr = remote_addr =>
                |store: &HimawariHotspotStore| {
                    for hotspots in store.iter_old_to_new(){
                        let remote_addr = remote_addr.clone();
                        let ws_msg = WsMsg::json( HimawariHotspotService::mod_path(), "hotspots", hotspots)?;
                        hself.try_send_msg( SendWsMsg{remote_addr,ws_msg})?;
                    }
                    Ok(())
                }
            };
            self.hupdater.send_msg( ExecSnapshotAction(action)).await?;
        }

        Ok(())
    }
}
