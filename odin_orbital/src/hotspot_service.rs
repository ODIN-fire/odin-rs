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

use std::net::SocketAddr;
use std::any::type_name;
use async_trait::async_trait;

use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_cesium::ImgLayerService;

use crate::{ load_asset, actor::{HotspotActorData, OrbitalHotspotActorMsg}};

pub struct OrbitalSat {
    name: String,
    hupdater: ActorHandle<OrbitalHotspotActorMsg>
}

pub struct OrbitalHotspotService {
    satellites: Vec<OrbitalSat>
}

impl OrbitalHotspotService {
    pub fn new (satellites: Vec<OrbitalSat>)->Self { OrbitalHotspotService { satellites } }
    pub fn mod_path()->&'static str { type_name::<Self>() }
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

        Ok(())
    }

    async fn data_available (&mut self, hself: &ActorHandle<SpaServerMsg>, has_connections: bool, sender_id: &str, data_type: &str) -> OdinServerResult<bool> {
        let mut is_our_data = false;

        if let Some(hupdater) = self.satellites.iter().find( |s| *s.hupdater.id == sender_id).map( |s| &s.hupdater) {
            if data_type == type_name::<HotspotActorData>() {
                if has_connections {
                    /* 
                    let action = dyn_dataref_action!( 
                        let hself: ActorHandle<SpaServerMsg> = hself.clone() => 
                        |data: &HotspotActorData| { send_hs_data( hself, None, data) }

                    );
                    hupdater.send_msg( ExecSnapshotAction(action)).await?;
                    */
                }
                is_our_data = true;
            }
        }

        Ok(is_our_data)
    }

    async fn init_connection (&mut self, hself: &ActorHandle<SpaServerMsg>, is_data_available: bool, conn: &mut SpaConnection) -> OdinServerResult<()> {
        if is_data_available {
            let remote_addr = conn.remote_addr;
            for sat in &self.satellites {
                /* 
                let action = dyn_dataref_action!{ 
                    let hself: ActorHandle<SpaServerMsg> = hself.clone(), 
                    let remote_addr: SocketAddr = remote_addr => 
                    |data: &HotspotActorData| { send_hs_data( hself, Some(remote_addr), data) }
                };
                sat.hupdater.send_msg( ExecSnapshotAction(action)).await?;
                */
            }
        }

        Ok(())
    }
}

fn send_hs_data (hself: &ActorHandle<SpaServerMsg>, remote_addr: Option<SocketAddr>, data: &HotspotActorData) -> std::result::Result<(),odin_action::OdinActionFailure> {
/* 
    let data = WsMsg::json( OrbitalHotspotService::mod_path(), "hotspots", hotspots)?;

    if let Some(remote_addr) = remote_addr {
        hself.try_send_msg( SendWsMsg{remote_addr,data})?;
    } else {
        hself.try_send_msg( BroadcastWsMsg{data})?;
    }
*/
    Ok(())
}