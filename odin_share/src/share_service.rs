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
use core::str;
use std::{sync::Arc,fmt::Debug, fs::File, io::BufReader, path::{Path, PathBuf},collections::HashMap, any::type_name, net::SocketAddr};
use serde::{Serialize,Deserialize};
use bytes::Bytes;
use crate::{dyn_shared_store_action, SharedStore, SharedStoreValueConstraints, load_asset,
    actor::{ExecSnapshotAction, SharedStoreActorMsg}
};

/// the generic wrapper type for shared items. This is what we keep in a SharedStore
/// TODO - should we add a concept of ownership here? if so it has to refer to users, not remote_addr (which might be transient)
#[derive(Serialize,Deserialize,Clone,Debug,PartialEq)]
#[serde(tag = "type")]
pub enum SharedItem {
    // geospatial types
    Point2D ( SharedItemValue<LatLon> ),
    Point3D ( SharedItemValue<GeoPos> ),
    Polyline ( SharedItemValue<Vec<LatLon>> ),

    // primitive types
    U64 ( SharedItemValue<u64> ),
    F64 ( SharedItemValue<f64> ),
    String ( SharedItemValue<String> ),

    /// a generic catch-all for structured data we only store as JSON source
    Json ( SharedItemValue<String>) 

    //... and more to follow
}

/// this is the wrapper type that adds the meta information to the payload, hence it is generic in the payload type
/// Note that we don't store the key - SharedItemValues are always accessed through their containing store, i.e.
/// storing the key would be redundant.
/// Note also that we keep the data in an Arc so that values can be efficiently cloned and we don't suffer from potential
/// enum variant size disparity
#[derive(Serialize,Deserialize,Clone,Debug,PartialEq)]
#[serde(bound = "T: for<'a> serde::Deserialize<'a>")]
pub struct SharedItemValue <T> 
    where T: SharedStoreValueConstraints
{
    pub comment: Option<String>,
    pub owner: Option<String>,
    pub data: Arc<T>
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
        spa_builder
            .add( build_service!( => UiService::new()))
            .add( build_service!( => WsService::new()))
    }

    fn add_components(&self, spa: &mut SpaComponents) -> OdinServerResult<()> {
        spa.add_assets(self_crate!(), load_asset);
        spa.add_module(asset_uri!("odin_share_config.js"));
        spa.add_module(asset_uri!("odin_share.js"));

        Ok(())
    }

    async fn init_connection( &mut self, hself: &ActorHandle<SpaServerMsg>, is_data_available: bool, conn: &mut SpaConnection) -> OdinServerResult<()> {
        // we provide the schema as JS code in the share_config.js module
        if is_data_available {
            let action = dyn_shared_store_action!( 
                let hself: ActorHandle<SpaServerMsg> = hself.clone(),
                let remote_addr: SocketAddr = conn.remote_addr => 
                |store as &dyn SharedStore<SharedItem>| {
                    let json = store.to_json()?;
                    let msg = ws_msg_from_json(ShareService::mod_path(), "initSharedItems", &json);
                    hself.try_send_msg( SendWsMsg{ remote_addr: *remote_addr, data: msg});
                    Ok(())
                }
            );

            self.hstore.send_msg( ExecSnapshotAction(action)).await?
        }

        Ok(())
    }

    // although it is unlikely the store will be initialized *after* we got connections we still want to support
    // store types that are remote or have to be synced externally (which implies network latency)
    async fn data_available (&mut self, hself: &ActorHandle<SpaServerMsg>, has_connections: bool,
                             sender_id: &str, data_type: &str) -> OdinServerResult<bool> {
        Ok(true)
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
