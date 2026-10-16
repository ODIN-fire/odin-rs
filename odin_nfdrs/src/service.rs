/*
 * Copyright 2026 Peter Mehlitz
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *    http://apache.org
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */
#![allow(unused)]

use std::{net::SocketAddr, sync::Arc, path::{Path,PathBuf}};
use axum::{
    routing::get,
    extract::{Path as AxumPath},
    response::Response,
};
use async_trait::async_trait;
use serde::Serialize;
use serde_json;

use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_cesium::ImgLayerService;

use crate::{
    GRID_DIR, load_asset,
    actor::{ExecSnapshotAction, NfdrsActorMsg, NfdrsStore, RemoveNfdrsRun},
    grid::{NfdrsField, NfdrsRequest}
};

/// max forecast hours we accept for client requests
pub const MAX_HOURS: u32 = 72;

/// a SpaService that lets clients request NFDRS grid products (hourly PNGs of fuel moisture and fire danger
/// indices for a given area) from a NfdrsActor and serves the computed products
pub struct NfdrsService {
    hactor: ActorHandle<NfdrsActorMsg>
}

impl NfdrsService {
    pub fn new (hactor: ActorHandle<NfdrsActorMsg>)->Self {
        NfdrsService { hactor }
    }

    async fn data_handler (path: AxumPath<String>) -> Response {
        compressable_file_response( GRID_DIR.as_path(), path.as_str(), "NFDRS data not found")
    }

    /// check request values we can verify without the actor. Returns an error message if invalid
    fn check_request (req: &NfdrsRequest)->Option<String> {
        if !req.has_valid_bbox() {
            Some( format!("invalid area: {:?}", req.bbox))
        } else if req.n_hours == 0 || req.n_hours > MAX_HOURS {
            Some( format!("invalid number of hours: {} (max {})", req.n_hours, MAX_HOURS))
        } else {
            None
        }
    }
}

#[async_trait]
impl SpaService for NfdrsService {

    fn add_dependencies (&self, spa_builder: SpaServiceList) -> SpaServiceList {
        spa_builder
            .add( build_service!( => ImgLayerService::new()))
    }

    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()>  {
        spa.add_assets( self_crate!(), load_asset);

        spa.add_module( asset_uri!( "odin_nfdrs_config.js"));
        spa.add_module( asset_uri!( "odin_nfdrs.js" ));

        spa.add_route( |router, spa_server_state| {
            router.route( &format!("/{}/nfdrs-data/{{*unmatched}}", spa_server_state.name.as_str()), get({
                move |path| Self::data_handler(path)
            }))
        });

        Ok(())
    }

    // since we don't load any data upon start we don't need a data_available() function

    async fn init_connection (&mut self, hself: &ActorHandle<SpaServerMsg>, is_data_available: bool, conn: &mut WsConnection) -> OdinServerResult<()> {
        let remote_addr = conn.remote_addr;

        // color maps first so that the client can display products of the subsequent snapshot
        hself.try_send_msg( SendWsMsg{ remote_addr, ws_msg: get_color_maps_msg() });

        let action = dyn_dataref_action!{
            let hself: ActorHandle<SpaServerMsg> = hself.clone(),  // this is the server handle
            let remote_addr: SocketAddr = remote_addr =>
            |store: &NfdrsStore| {                                 // this gets executed by the NfdrsActor
                let remote_addr = remote_addr.clone();
                let ws_msg = store.get_json_snapshot_msg();         // send even if empty so that client can reset
                Ok( hself.try_send_msg( SendWsMsg{remote_addr,ws_msg})? )
            }
        };
        self.hactor.send_msg( ExecSnapshotAction(action)).await?;

        Ok(())
    }

    async fn handle_ws_msg (&mut self,
        hself: &ActorHandle<SpaServerMsg>, remote_addr: &SocketAddr, ws_msg_parts: &WsMsgParts
    ) -> OdinServerResult<WsMsgReaction>
    {
        if ws_msg_parts.mod_path == NfdrsService::mod_path() {
            match ws_msg_parts.msg_type {
                "request" => {
                    match serde_json::from_str::<NfdrsRequest>( ws_msg_parts.payload) {
                        Ok(req) => {
                            info!("received NFDRS request from {}: {:?}", remote_addr, req);
                            if let Some(err_msg) = Self::check_request( &req) {
                                send_error( hself, remote_addr, err_msg);
                            } else {
                                self.hactor.send_msg( req).await?;
                            }
                        }
                        Err(e) => warn!("ignoring malformed NFDRS request: {}", e)
                    }
                }
                "remove" => {
                    match serde_json::from_str::<RemoveNfdrsRun>( ws_msg_parts.payload) {
                        Ok(req) => { self.hactor.send_msg( req).await?; }
                        Err(e) => warn!("ignoring malformed NFDRS remove request: {}", e)
                    }
                }
                _ => { warn!("ignoring unknown websocket message: {}", ws_msg_parts.msg_type) }
            }
        }
        Ok( WsMsgReaction::None )
    }
}

//--- outgoing websocket messages that are not related to NfdrsRuns

fn send_error (hself: &ActorHandle<SpaServerMsg>, remote_addr: &SocketAddr, msg: String) {
    warn!("{}", msg);
    let json = serde_json::to_string( &ErrorMsg{ msg }).unwrap_or_else( |_| "{}".to_string());
    hself.try_send_msg( SendWsMsg{ remote_addr: *remote_addr, ws_msg: ws_msg_from_json( NfdrsService::mod_path(), "error", &json) });
}

#[derive(Serialize)]
struct ErrorMsg { msg: String }

#[derive(Serialize)]
struct ColorMapEntryMsg { color: String, code: String, descr: &'static str }

#[derive(Serialize)]
struct ColorMapMsg { field: String, descr: &'static str, unit: &'static str, entries: Vec<ColorMapEntryMsg> }

fn get_color_maps_msg ()->String {
    let cms: Vec<ColorMapMsg> = NfdrsField::all().iter().map( |f| {
        let entries = f.color_map().into_iter().map( |ce| ColorMapEntryMsg{ color: ce.css_color(), code: ce.code, descr: ce.descr }).collect();
        ColorMapMsg { field: f.to_string(), descr: f.description(), unit: f.unit(), entries }
    }).collect();

    let json = serde_json::to_string( &cms).unwrap_or_else( |_| "[]".to_string());
    ws_msg_from_json( NfdrsService::mod_path(), "colorMaps", &json)
}
