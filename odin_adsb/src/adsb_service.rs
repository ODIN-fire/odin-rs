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

use std::{net::SocketAddr, time::Duration};
use axum::{
    http::{Uri,StatusCode},
    body::Body,
    routing::{Router,get},
    extract::{Path as AxumPath},
    response::{Response,IntoResponse},
};
use async_trait::async_trait;

use odin_actor::prelude::*;
use odin_common::json_writer::JsonWriter;
use odin_server::prelude::*;
use odin_cesium::ImgLayerService;

use crate::{load_asset, AircraftStore, actor::{AdsbActor, AdsbActorMsg, ExecSnapshotAction}};

pub struct AdsbService {
    actors: Vec<ActorHandle<AdsbActorMsg>>
}

impl AdsbService {
    pub fn new (actors: Vec<ActorHandle<AdsbActorMsg>>)->Self {
        AdsbService{actors}
    }
}

#[async_trait]
impl SpaService for AdsbService {

    fn add_dependencies (&self, spa_builder: SpaServiceList) -> SpaServiceList {
        spa_builder.add( build_service!( => ImgLayerService::new()))
    }

    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()>  {
        spa.add_assets( self_crate!(), load_asset);

        spa.add_module( asset_uri!( "odin_adsb_config.js"));
        spa.add_module( asset_uri!( "odin_adsb.js" ));

        Ok(())
    }

    // no data_available as this is highly dynamic (we could send it once the connector is live)

    async fn init_connection (&mut self, hself: &ActorHandle<SpaServerMsg>, is_data_available: bool, conn: &mut SpaConnection) -> OdinServerResult<()> {
        let remote_addr = conn.remote_addr;

        for hactor in &self.actors {
            let action = dyn_dataref_action!{
                let hself: ActorHandle<SpaServerMsg> = hself.clone(),  // this is the server handle
                let remote_addr: SocketAddr = remote_addr => 
                |store: &AircraftStore| {                              // this gets executed by the AdsbActor 
                    let remote_addr = remote_addr.clone();
                    let ws_msg = store.get_json_snapshot_msg();
                    Ok( hself.try_send_msg( SendWsMsg{remote_addr,ws_msg})? )
                }
            };
            hactor.send_msg( ExecSnapshotAction(action)).await?; // send the action requests to the AdsbActors
        }

        Ok(())
    }
}