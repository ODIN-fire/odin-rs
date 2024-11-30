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

//! the share_service module implements storage, update and distribution of typed data items and of selected user
//! interactions across all micro-services of an application. Keeping with the general philosophy of odin-rs
//! shareable items are statically typed

// TODO - this should be compatible with a potential future implementation of RACE/SHARE
// https://nasarace.github.io/race/design/share.html which supports data distribution of tabular data within
// network nodes with a tree topology

use odin_server::{ prelude::*, errors::op_failed,};
use async_trait::async_trait;
use odin_actor::prelude::*;
use odin_build::prelude::*;
use odin_common::{define_serde_struct, geo::{GeoBoundingBox, GeoPos, LatLon}};
use std::{sync::Arc,fmt::Debug, fs::File, io::BufReader, path::{Path, PathBuf},collections::HashMap, any::type_name, net::SocketAddr};
use serde::{Serialize,Deserialize};
use crate::{load_asset,actor::{SharedStoreActorMsg, SharedStoreValueConstraints}};

/// the generic wrapper type for shared items
/// TODO - should we add a concept of ownership here? remoteAddr seems too fragile
#[derive(Serialize,Deserialize,Clone,Debug)]
pub enum SharedItem {
    Point2D ( SharedItemValue<LatLon> ),
    Point3D ( SharedItemValue<GeoPos> )
}

#[derive(Serialize,Deserialize,Clone,Debug)]
#[serde(bound = "T: for<'a> serde::Deserialize<'a>")]
pub struct SharedItemValue <T> 
    where T: SharedStoreValueConstraints
{
    key: Arc<String>,
    comment: Option<String>,
    owner: Option<String>,
    data: Arc<T>
}


/// micro service to share data between users and other micro-services. This is UI-less
pub struct ShareService {
    hstore: ActorHandle<SharedStoreActorMsg<SharedItem>>
}

impl ShareService 
{
    pub fn mod_path()->&'static str { type_name::<Self>() }

    pub fn new (hstore: ActorHandle<SharedStoreActorMsg<SharedItem>>) -> Self {
        //let data_dir = odin_build::data_dir().join("odin_server");
        ShareService { hstore }
    }
}

#[async_trait]
impl SpaService for ShareService {
    fn add_dependencies(&self, spa_builder: SpaServiceList) -> SpaServiceList {
        spa_builder.add(build_service!(WsService::new()))
    }

    fn add_components(&self, spa: &mut SpaComponents) -> OdinServerResult<()> {
        spa.add_assets(self_crate!(), load_asset);
        spa.add_module(asset_uri!("share.js"));

        Ok(())
    }

    async fn init_connection( &mut self, hself: &ActorHandle<SpaServerMsg>, is_data_available: bool, conn: &mut SpaConnection) -> OdinServerResult<()> {
        let remote_addr = conn.remote_addr;

        /*
        let point2d_list = &self.point2d;
        let data = ws_msg!(MOD_PATH, point2d_list).to_json()?;
        hself.try_send_msg(SendWsMsg { remote_addr, data })?;

        let point3d_list = &self.point3d;
        let data = ws_msg!(MOD_PATH, point3d_list).to_json()?;
        hself.try_send_msg(SendWsMsg { remote_addr, data })?;

        let bbox_list = &self.bbox;
        let data = ws_msg!(MOD_PATH, bbox_list).to_json()?;
        hself.try_send_msg(SendWsMsg { remote_addr, data })?;
        */

        Ok(())
    }

    // "setLatLon": { "key": "/incidents/czu/origin", "comment": "blah", "data": {"lat": 37.123, "lon": -122.12} }

    /// this is how we get data from clients. Called from ws input task of respective connection
    async fn handle_ws_msg (&mut self, 
        hself: &ActorHandle<SpaServerMsg>, remote_addr: &SocketAddr, ws_msg_parts: &WsMsgParts) -> OdinServerResult<WsMsgReaction> 
    {
        if ws_msg_parts.mod_path == ShareService::mod_path() {
            match ws_msg_parts.msg_type {
                "setLatLon" => {
                    if let Ok(shared_item) = serde_json::from_str::<SharedItem>(ws_msg_parts.payload) {
                    }
                }
                _ => {
                    warn!("ignoring unknown websocket message {}", ws_msg_parts.msg_type)
                }
            }
        }

        Ok( WsMsgReaction::None )
    }
}
