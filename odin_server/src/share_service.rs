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

use crate::{errors::op_failed, prelude::*};
use crate::{load_asset, load_config, spa::WsMsgReaction};
use async_trait::async_trait;
use odin_actor::prelude::*;
use odin_build::prelude::*;
use odin_common::{define_serde_struct, fs, geo::{GeoBoundingBox, GeoPos, LatLon}};
use ron;
use serde::{Deserialize, Serialize};
use serde_json;
use std::{fmt::Debug, fs::File, io::BufReader, path::{Path, PathBuf},collections::HashMap, any::type_name};
use glob::Pattern;


/// some piece of data that can be shared through this micro service
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Shared {
    /// id is a path that encodes <crate>/<module>/.../<name> and can be matched against a glob pattern
    pub key: String,

    /// the serialized value
    pub data: String,
}

impl Shared {
    pub fn is_matching(&self, glob: &Pattern) -> bool {
        glob.matches(&self.key)
    }
    pub fn has_prefix(&self, prefix: &str)-> bool {
        self.key.starts_with(prefix)
    }
}

/// incoming messages we can receive over the web socket
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IncomingSharedWsMsg {
    Add(Shared),
    Update(Shared),
    Remove{key: String},
}

/// micro service to share data between users and other micro-services. This is UI-less
pub struct ShareService {
}

impl ShareService {
    pub fn mod_path()->&'static str { type_name::<Self>() }

    pub fn new() -> Self {
        let data_dir = odin_build::data_dir().join("odin_server");
        ShareService {}
    }

    /*
    pub fn add_point2d(&mut self, v: &Shared<LatLon>)->bool {
        if point2d.contains(|p| p.is_matching( &v.name, &v.category, &v.group.as_ref())) { return false }
        point2d.push( v.clone());
        true
    }
    */
}

#[async_trait]
impl SpaService for ShareService {
    fn add_dependencies(&self, spa_builder: SpaServiceList) -> SpaServiceList {
        spa_builder.add(build_service!(WsService::new()))
    }

    fn add_components(&self, spa: &mut SpaComponents) -> OdinServerResult<()> {
        spa.add_assets(self_crate!(), load_asset);
        spa.add_module(asset_uri!("shared.js"));

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

    /// this is how we get data from clients. Called from ws input task of respective connection
    fn handle_incoming_ws_msg (&mut self, handler_key: &str, payload_name: &str, payload: &str) -> WsMsgReaction {
        WsMsgReaction::None
    }
}
