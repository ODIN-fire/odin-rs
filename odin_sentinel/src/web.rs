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

use std::net::SocketAddr;

use async_trait::async_trait;

use odin_build::prelude::*;
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_cesium::CesiumService;

use crate::{load_asset, ExecSnapshotAction, SentinelActorMsg, SentinelStore};

/// SpaService to show sentinel infos on a cesium display
pub struct SentinelService {
    hsentinel: ActorHandle<SentinelActorMsg> // our data source
    // ..possibly more to follow
}

impl SentinelService {
    pub fn new (hsentinel: ActorHandle<SentinelActorMsg>)->Self { 
        SentinelService{hsentinel}
    }
}

#[async_trait]
impl SpaService for SentinelService {
    fn add_dependencies (&self, spa_builder: SpaServiceListBuilder) -> SpaServiceListBuilder {
        spa_builder.add( build_service!( CesiumService::new()))

    }

    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()>  {
        spa.add_assets( self_crate!(), load_asset);
        spa.add_module( asset_uri!("odin_sentinel_config.js"));
        spa.add_module( asset_uri!("odin_sentinel.js"));

        Ok(())
    }

    // send an ExecSnapshotAction to the SentinelActor to send a JSON websocket message to the new connection
    async fn init_connection (&self, hself: &ActorHandle<SpaServerMsg>, conn: &mut SpaConnection) -> OdinServerResult<()> {
        let remote_addr = conn.remote_addr;
        let action = dyn_dataref_action!( hself.clone(): ActorHandle<SpaServerMsg>, remote_addr: SocketAddr => |data: &SentinelStore| {
            let data = map_action_err( data.to_json(false))?;
            let remote_addr = remote_addr.clone();
            hself.try_send_msg( SendWsMsg{remote_addr,data})
        });
        Ok( self.hsentinel.send_msg( ExecSnapshotAction(action)).await? )
    }
}