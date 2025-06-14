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

use std::{net::SocketAddr,fs,sync::Arc,path::{Path,PathBuf}};
use std::any::type_name;
use async_trait::async_trait;
use axum::{response::Response, routing::{Router,get}, extract::{Path as AxumPath}};
use serde_json;

use odin_actor::prelude::*;
use odin_common::json_writer::JsonWriter;
use odin_server::prelude::*;
use odin_cesium::ImgLayerService;

use crate::{
    actor::{AddWindClient, RemoveWindClient, ExecSnapshotAction, WindActorMsg, WindRegion}, 
    forecast_regions_to_json, load_asset, ForecastStore, PKG_CACHE_DIR
};

pub struct WindService {
    hwind: ActorHandle<WindActorMsg>,
    // TODO
}

impl WindService {
    pub fn new (hwind: ActorHandle<WindActorMsg>)->Self {
        WindService { hwind }
    }

    async fn data_handler (path: AxumPath<String>) -> Response {
        // this is served from our cache dir as compressed CSV or JSON files
        odin_server::compressable_file_response::<&Path>( PKG_CACHE_DIR.as_ref(), path.as_str(), "windninja data not found")
    }
}

#[async_trait]
impl SpaService for WindService {
    fn add_dependencies (&self, spa_builder: SpaServiceList) -> SpaServiceList {
        spa_builder
            .add( build_service!( => UiService::new()))
            .add( build_service!( => WsService::new()))
            .add( build_service!( => ImgLayerService::new()))
    }

    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()>  {
        spa.add_assets( self_crate!(), load_asset);

        spa.add_module( asset_uri!( "odin_wind_config.js"));
        spa.add_module( asset_uri!( "odin_wind.js" ));

        //--- the visualization support modules
        spa.add_module( asset_uri!( "windfield.js" ));
        spa.add_module( asset_uri!( "wind-particles/windUtils.js" ));
        spa.add_module( asset_uri!( "wind-particles/particleSystem.js" ));
        spa.add_module( asset_uri!( "wind-particles/particlesComputing.js" ));
        spa.add_module( asset_uri!( "wind-particles/particlesRendering.js" ));

        spa.add_route( |router, spa_server_state| {
            router.route( &format!("/{}/wind-data/{{*unmatched}}", spa_server_state.name.as_str()), get(Self::data_handler))
        });

        Ok(())
    }

    // we don't have a data_available() since we only produce in response to Add/RemoveClientRequest messages

    async fn init_connection (&mut self, hself: &ActorHandle<SpaServerMsg>, is_data_available: bool, conn: &mut SpaConnection) -> OdinServerResult<()> {
        let remote_addr = conn.remote_addr;

        // we send this un-conditionally since we don't know if the wn actor has/produces forecasts
        let action = dyn_dataref_action!{ 
            let hself: ActorHandle<SpaServerMsg> = hself.clone(), 
            let remote_addr: SocketAddr = remote_addr => 
            |data: &ForecastStore| { 
                if !data.is_empty() {
                    let json = forecast_regions_to_json(data);
                    let ws_msg = ws_msg_from_json( WindService::mod_path(), "forecastRegions", &json);
                    let remote_addr = *remote_addr;
                    hself.send_msg( SendWsMsg{remote_addr,ws_msg}).await;
                }
                Ok(())
            }
        };
        self.hwind.send_msg( ExecSnapshotAction(action)).await?;

        Ok(())
    }

    /// this is how we get regions from clients. Called from ws input task of respective connection
    async fn handle_ws_msg (&mut self, 
        hserver: &ActorHandle<SpaServerMsg>, remote_addr: &SocketAddr, ws_msg_parts: &WsMsgParts) -> OdinServerResult<WsMsgReaction> 
    {
        if ws_msg_parts.mod_path == WindService::mod_path() {
            match ws_msg_parts.msg_type {
                "addWindClient" => {
                    let wn_region: WindRegion = serde_json::from_str(&ws_msg_parts.payload)?;
                    info!("got addWindClient {:?} from {:?}", wn_region.name, remote_addr);
                    self.hwind.send_msg( AddWindClient{wn_region,remote_addr: Some(remote_addr.clone())}).await;
                }
                "removeWindClient" => {
                    let wn_region: WindRegion = serde_json::from_str(&ws_msg_parts.payload)?;
                    info!("got removeWindClient {:?} from {:?}", wn_region.name, remote_addr);
                    let region = Some(wn_region.name);
                    let remote_addr = Some(remote_addr.clone());
                    self.hwind.send_msg( RemoveWindClient{region,remote_addr}).await;
                }
                _ => {}
            }
        }

        Ok(WsMsgReaction::None)
    }

    // let actor know about dropped connection - it might have been a subscriber
    // unfortunately we have to delegate this to the actor since we don't know if 'addWindClient' requests get rejected
    async fn remove_connection (&mut self, hself: &ActorHandle<SpaServerMsg>, remote_addr: &SocketAddr) -> OdinServerResult<()> {
        self.hwind.send_msg( RemoveWindClient{region:None, remote_addr: Some(remote_addr.clone())}).await;
        Ok(())
    }
}