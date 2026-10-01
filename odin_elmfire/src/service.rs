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

use std::{net::SocketAddr,any::type_name,fs, time::Duration, sync::Arc, path::{Path,PathBuf}};
use axum::{
    http::{Uri,StatusCode},
    body::Body,
    routing::{Router,get},
    extract::{Path as AxumPath},
    response::{Response,IntoResponse},
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json;
use chrono::{DateTime,Utc};

use odin_common::{
    angle::Angle360,
    datetime::{from_epoch_millis, de_from_epoch_millis},
    geo::GeoPoint,
    json_writer::JsonWriter
};
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_cesium::ImgLayerService;
use uom::si::{velocity::mile_per_hour,length::kilometer,ratio::percent};
use uom::si::f64::{Velocity,Length,Ratio};

use crate::{
    CACHE_DIR, DATA_DIR, ElmFire, ElmfireRun, FIXED_DIR, FixedInputStep, NML_RE, SPOTTING_DIR,
    actor::{ElmfireActorMsg, ElmfireStore, ExecSnapshotAction},
    errors::{OdinElmfireError, Result},
    fixed, get_contents, get_saved_list, load_asset, remove_contents, save_contents
};

pub struct ElmfireService {
    hactor: ActorHandle<ElmfireActorMsg>,
    spotting_list: Vec<String>,
    fixed_list: Vec<String>
}

impl ElmfireService {
    pub fn new (hactor: ActorHandle<ElmfireActorMsg>)->Self {
        let spotting_list: Vec<String> = if let Ok(list) = get_saved_list( SPOTTING_DIR.as_path(), &NML_RE) { list } else { Vec::new() };
        let fixed_list: Vec<String> = if let Ok(list) = get_saved_list( FIXED_DIR.as_path(), &NML_RE) { list } else { Vec::new() };
        ElmfireService{hactor, spotting_list, fixed_list}
    }

    async fn data_handler (path: AxumPath<String>, dir: Arc<PathBuf>) -> Response {
        compressable_file_response( CACHE_DIR.as_path(), path.as_str(), "fire data not found")
    }

    async fn spotting_handler (path: AxumPath<String>) -> Response {
        let fname = format!("{}.nml", path.as_str());
        compressable_file_response( SPOTTING_DIR.as_path(), &fname, "spotting file not found")
    }

    async fn fixed_handler (path: AxumPath<String>) -> Response {
        let fname = format!("{}.nml", path.as_str());
        compressable_file_response( FIXED_DIR.as_path(), &fname, "wx/fuel file not found")
    }
}

fn get_stored_list_ws_msg (list: &[String], msg_type: &'static str)->String {
    let mut w = JsonWriter::with_capacity(1024);
    w.write_array( |w| {
        for s in list {
            w.write_value(s)
        }
    });
    ws_msg_from_json( ElmfireService::mod_path(), msg_type, w.as_str())
}

#[async_trait]
impl SpaService for ElmfireService {

    fn add_dependencies (&self, spa_builder: SpaServiceList) -> SpaServiceList {
        spa_builder
            .add( build_service!( => CodeService::new()))
            .add( build_service!( => ImgLayerService::new()))
    }

    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()>  {
        spa.add_assets( self_crate!(), load_asset);

        spa.add_module( asset_uri!( "odin_elmfire_config.js"));
        spa.add_module( asset_uri!( "odin_elmfire.js" ));

        let dir = Arc::new(CACHE_DIR.clone());
        spa.add_route( |router, spa_server_state| {
            router.route( &format!("/{}/elmfire-data/{{*unmatched}}", spa_server_state.name.as_str()), get({
                move |path| Self::data_handler(path, dir.clone())
            }))
        });

        spa.add_route( |router, spa_server_state| {
            router.route( &format!("/{}/elmfire-spotting/{{*unmatched}}", spa_server_state.name.as_str()), get({
                move |path| Self::spotting_handler(path)
            }))
        });

        spa.add_route( |router, spa_server_state| {
            router.route( &format!("/{}/elmfire-fixed/{{*unmatched}}", spa_server_state.name.as_str()), get({
                move |path| Self::fixed_handler(path)
            }))
        });

        Ok(())
    }

    // since we don't load any data upon start we don't need a data_available() function

    async fn init_connection (&mut self, hself: &ActorHandle<SpaServerMsg>, is_data_available: bool, conn: &mut WsConnection) -> OdinServerResult<()> {
        let remote_addr = conn.remote_addr;

        let ws_msg = get_stored_list_ws_msg( &self.spotting_list, "spotting"); // send clients updated list
        hself.try_send_msg( SendWsMsg{remote_addr, ws_msg});

        let ws_msg = get_stored_list_ws_msg( &self.fixed_list, "fixed");
        hself.try_send_msg( SendWsMsg{remote_addr, ws_msg});

        let action = dyn_dataref_action!{
            let hself: ActorHandle<SpaServerMsg> = hself.clone(),  // this is the server handle
            let remote_addr: SocketAddr = remote_addr =>
            |store: &ElmfireStore| {                              // this gets executed by the ElmfireActor
                if !store.is_empty() {
                    let remote_addr = remote_addr.clone();
                    let ws_msg = store.get_json_snapshot_msg();
                    Ok( hself.try_send_msg( SendWsMsg{remote_addr,ws_msg})? )
                } else {
                    Ok(()) // nothing to do
                }
            }
        };

        // we don't know here if the actor has data so we need to send it
        self.hactor.send_msg( ExecSnapshotAction(action)).await?;

        Ok(())
    }

    async fn handle_ws_msg (&mut self,
        hself: &ActorHandle<SpaServerMsg>, remote_addr: &SocketAddr, ws_msg_parts: &WsMsgParts
    ) -> OdinServerResult<WsMsgReaction>
    {
        if ws_msg_parts.mod_path == ElmfireService::mod_path() {
            match ws_msg_parts.msg_type {
                "elmfire"  => { // create a new ElmFire request and send it to our actor
                    match serde_json::from_str::<ElmfireRequest>( ws_msg_parts.payload) {
                        Ok(efr) => {
                            info!("received ElmfireRequest from {} : {}", remote_addr, efr.name);
                            match ElmFire::try_from(efr) {
                                Ok(ef) => {
                                    self.hactor.send_msg(ef).await;
                                }
                                Err(e) => { warn!("failed to create ElmFire from request: {}", e) }
                            }
                        }
                        Err(e) => {
                            warn!("ignoring malformed ElmFire request: {}", e)
                        }
                    }
                }

                // TODO - factor out store_/del_content

                "storeSpotting" => {
                    store_content( ws_msg_parts.payload, SPOTTING_DIR.as_path(), &mut self.spotting_list, hself, "spotting");
                }
                "delSpotting" => {
                    del_content( ws_msg_parts.payload, SPOTTING_DIR.as_path(), &mut self.spotting_list, hself, "spotting");
                }
                _ => { warn!("ignoring unknown websocket message: {}", ws_msg_parts.msg_type) }
            }
        }
        Ok( WsMsgReaction::None )
    }
}

fn store_content<P: AsRef<Path>> (msg: &str, dir: P, list: &mut Vec<String>, hself: &ActorHandle<SpaServerMsg>, msg_type: &'static str) {
    match serde_json::from_str::<StoreContent>( msg) {
        Ok(sc) => {
            info!("store '{}'", sc.name);
            match save_contents( dir, &sc.name, &sc.source) {
                Ok(()) => {
                    list.push( sc.name);
                    list.sort();
                    let ws_msg = get_stored_list_ws_msg( list, msg_type); // send clients updated list
                    hself.try_send_msg( BroadcastWsMsg{ws_msg});
                }
                Err(e) => { warn!("failed to save data: {}", e) }
            }
        }
        Err(e) => { warn!("ignoring malformed SaveContent request: {}", e) }
    }
}

fn del_content<P: AsRef<Path>> (msg: &str, dir: P, list: &mut Vec<String>, hself: &ActorHandle<SpaServerMsg>, msg_type: &'static str) {
    match serde_json::from_str::<DelContent>( msg) {
        Ok(dc) => {
            if let Some(idx) = list.iter().position(|s| s.as_str() == dc.name.as_str()) {
                info!("delete settings for '{}'", dc.name);
                list.remove(idx);
                match remove_contents( dir, &dc.name) {
                    Ok(()) => {}
                    Err(e) => { warn!("failed to delete data: {}", e) }
                }
                let ws_msg =  get_stored_list_ws_msg( list, msg_type); // send clients updated list
                hself.try_send_msg( BroadcastWsMsg{ws_msg});
            }
        }
        Err(e) => { warn!("ignoring malformed DelContent request: {}", e) }
    }
}

//-- input messages (received from the web socket)

#[derive(Deserialize, Debug)]
#[serde(rename_all="camelCase")]
struct ElmfireRequest {
    id: String,
    name: String,
    #[serde(deserialize_with="de_from_epoch_millis")]
    date: DateTime<Utc>, // of first ignition
    pos: GeoPoint,
    n_hours: u32,
    size: f64,          // of bbox sides in km
    compute_wind: bool, // compute micro-grid wind
    ignitions: String,
    fixed: Option<String>, // use fixed wx and fuel values instead of wx reports/forecast
    spotting: Option<String>,
    barriers: Option<String>
}

impl TryFrom<ElmfireRequest> for ElmFire {
    type Error = OdinElmfireError;

    fn try_from(req: ElmfireRequest) -> Result<Self> {
        Ok( ElmFire::new(
            req.id,
            req.name,
            req.pos,
            Length::new::<kilometer>(req.size),
            req.date,
            req.n_hours,
            req.compute_wind,
            req.ignitions,
            req.fixed,
            req.spotting,
            req.barriers
        )? )
    }
}


#[derive(Deserialize,Debug)]
struct StoreContent {
    name: String,
    source: String
}

#[derive(Deserialize,Debug)]
struct DelContent {
    name: String
}
