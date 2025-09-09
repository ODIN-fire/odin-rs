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

use std::{any::type_name, collections::HashMap, fmt::Debug, fs::{self,File}, io::BufReader, net::SocketAddr, ops::Index, path::{Path, PathBuf}, sync::Arc};
use serde::{Serialize,Deserialize};
use serde_json::{Value as JsonValue, json};
use axum::{
    http::StatusCode,
    routing::{Router,get},
    response::{Response,IntoResponse}
};
use bytes::Bytes;
use odin_cesium::CesiumService;
use odin_server::{ errors::op_failed, prelude::*};
use async_trait::async_trait;
use odin_actor::prelude::*;
use odin_build::{pkg_data_dir, prelude::*};
use odin_common::{define_serde_struct, geo::{GeoPoint, GeoPoint3, GeoLine, GeoLineString, GeoRect, GeoPolygon, GeoCircle}};
use crate::{
    actor::{ExecSnapshotAction, SetSharedStoreEntry, RemoveSharedStoreEntry, SharedStoreActor, SharedStoreActorMsg, SharedStoreChange, SharedStoreUpdate}, 
    dyn_shared_store_action, load_asset, SharedStore, SharedStoreValueConstraints, shared_store_action, SharedStoreAction,
};


/// the generic wrapper type for shared items. This is what we keep in a SharedStore
/// the variants should eventually cover the GeoJSON types (the geo_types geometries), 3d points, the basic primitive types (String, f64 etc),
/// and a generic Json type that just stores the value as a JSON string
#[derive(Serialize,Deserialize,Clone,Debug)]
#[serde(tag = "type")]
pub enum SharedItemType {
    // geospatial types
    GeoPoint ( SharedItemValue<GeoPoint> ),
    GeoPoint3 ( SharedItemValue<GeoPoint3> ),
    GeoLine ( SharedItemValue<GeoLine> ),
    GeoLineString ( SharedItemValue<GeoLineString> ),
    GeoRect ( SharedItemValue<GeoRect> ),
    GeoPolygon (SharedItemValue<GeoPolygon> ),
    GeoCircle (SharedItemValue<GeoCircle> ),

    // primitive types
    U64 ( SharedItemValue<u64> ),
    F64 ( SharedItemValue<f64> ),
    String ( SharedItemValue<String> ),

    /// a generic catch-all for structured data we only store as JSON source
    Json ( SharedItemValue<String>) // TODO - should we store this as a serde_json::Value ? This would make it harder to clone

    //... and more to follow
}

/// this is the wrapper type that adds the common meta information to the payload, hence it is generic in the payload type
/// Note that we don't store the key - SharedItemValues are always accessed through their containing store, i.e.
/// storing the key would be redundant.
/// Note also that we keep the data in an Arc so that values can be efficiently cloned and we don't suffer from potential
/// enum variant size disparity
#[derive(Serialize,Deserialize,Clone,Debug)]
#[serde(bound = "T: for<'a> serde::Deserialize<'a>")]
pub struct SharedItemValue <T> 
    where T: SharedStoreValueConstraints
{
    pub comment: Option<String>,
    pub owner: Option<String>,
    pub data: Arc<T>
}

/// keep track of publisher/subscribers for user roles
struct RoleEntry {
    remote_addr: SocketAddr,
    is_publishing: bool,
    subscribers: Vec<SocketAddr>, // conceptually this is a HashSet but the main use is iteration and cloning
}

impl RoleEntry {
    fn new (remote_addr: SocketAddr) -> Self {
        RoleEntry { remote_addr, is_publishing: false, subscribers: Vec::new() }
    }
    fn add_subscriber (&mut self, remote_addr: SocketAddr) {
        if !self.subscribers.contains( &remote_addr) {
            self.subscribers.push(remote_addr);
        }
    }
    fn remove_subscriber (&mut self, remote_addr: SocketAddr) {
        self.subscribers.retain( |ar| *ar != remote_addr);
    }

    fn json_value (&self, role: &str) -> JsonValue {
        json!({
            "role": role,
            "isPublishing": self.is_publishing,
            "nSubscribers": self.subscribers.len()
        })
    }
}

/// micro service to share data between users and other micro-services. This is UI-less
pub struct ShareService {
    schema: Arc<String>, // (JS) asset filename for schema to use by client
    hstore: ActorHandle<SharedStoreActorMsg<SharedItemType>>,
    user_roles: HashMap<String,RoleEntry>,
}

impl ShareService {
    pub fn new (schema: &str, hstore: ActorHandle<SharedStoreActorMsg<SharedItemType>>) -> Self {
        //let data_dir = odin_build::data_dir().join("odin_server");
        let user_roles = HashMap::new();
        let schema = Arc::new(schema.to_owned());

        ShareService { schema, hstore, user_roles }
    }

    fn get_user_roles_json (&self)->String {
        let a = JsonValue::Array( self.user_roles.iter().map(|e| e.1.json_value(&e.0)).collect());
        serde_json::to_string(&a).unwrap() // save to unwrap as we are explicitly constructing the JsonValue
    }

    async fn schema_handler (schema: Arc<String>) -> Response {
        if let Ok(data) = load_asset(schema.as_str()) {
            get_asset_response(schema.as_str(), data)

        } else {
            (StatusCode::NOT_FOUND, "schema not found").into_response()
        }
    }
}

#[async_trait]
impl SpaService for ShareService {
    fn add_dependencies(&self, spa_builder: SpaServiceList) -> SpaServiceList {
        spa_builder
            .add( build_service!( => UiService::new()))
            .add( build_service!( => WsService::new()))
            .add( build_service!( => CesiumService::new()))
    }

    fn add_components(&self, spa: &mut SpaComponents) -> OdinServerResult<()> {
        spa.add_module(asset_uri!("odin_share_config.js"));
        spa.add_module(asset_uri!("odin_share_schema.js")); // this is a virtual module - the physical file name is configured
        spa.add_module(asset_uri!("odin_share.js"));

        let schema = self.schema.clone();
        // note this is overlapping with our normal assets path so it has to be defined first
        spa.add_route( |router, spa_server_state| {
            router.route( &format!("/{}/asset/odin_share/odin_share_schema.js", spa_server_state.name.as_str()), get({
                move || Self::schema_handler(schema.clone())
            }))
        });

        spa.add_assets(self_crate!(), load_asset);

        Ok(())
    }

    async fn init_connection( &mut self, hself: &ActorHandle<SpaServerMsg>, is_data_available: bool, conn: &mut WsConnection) -> OdinServerResult<()> {
        // we provide the schema as JS code in the share_config.js module
        if is_data_available {
            let action = dyn_shared_store_action!( 
                let hself: ActorHandle<SpaServerMsg> = hself.clone(),
                // TODO - send current user_roles
                let remote_addr: SocketAddr = conn.remote_addr => 
                |store as &dyn SharedStore<SharedItemType>| {
                    let json = store.to_json()?;
                    let ws_msg = ws_msg_from_json(ShareService::mod_path(), "initSharedItems", &json);
                    hself.try_send_msg( SendWsMsg{ remote_addr: *remote_addr, ws_msg});
                    Ok(())
                }
            );

            self.hstore.send_msg( ExecSnapshotAction(action)).await?
        }

        let ws_msg = ws_msg_from_json( ShareService::mod_path(), "initExtRoles", &self.get_user_roles_json());
        hself.send_msg( SendWsMsg{remote_addr: conn.remote_addr, ws_msg}).await;

        Ok(())
    }

    async fn remove_connection (&mut self, hself: &ActorHandle<SpaServerMsg>, remote_addr: &SocketAddr) -> OdinServerResult<()> {
        let mut dropped_roles: Vec<String> = self.user_roles.iter().filter(|e| e.1.remote_addr == *remote_addr).map( |e| e.0.clone()).collect();
        self.user_roles.retain( |kr,vr| vr.remote_addr != *remote_addr);

        if !dropped_roles.is_empty() {
            let ws_msg = WsMsg::json( ShareService::mod_path(), "rolesDropped", dropped_roles)?;
            hself.send_msg( BroadcastWsMsg{ ws_msg}).await; // we can broadcast here since remote_addr is already removed from connections
        }

        // update the remaining roles that had remote_addr as a subscriber
        // TODO - this is potentially sending lots of messages if the lost connections had many subscriptions
        for e in self.user_roles.iter_mut() {
            let role = e.0;
            let mut role_entry = e.1;
            if let Some(idx) = role_entry.subscribers.iter_mut().position( |a| *a == *remote_addr){
                role_entry.subscribers.remove(idx);

                let ws_msg = WsMsg::json( ShareService::mod_path(), "updateRole", role_entry.json_value(role))?;
                hself.send_msg( SendWsMsg{ remote_addr: role_entry.remote_addr, ws_msg}).await;
            }
        }

        Ok(())
    }

    // although it is unlikely the store will be initialized *after* we got connections we still want to support
    // store types that are remote or have to be synced externally (which implies network latency)
    async fn data_available (&mut self, hself: &ActorHandle<SpaServerMsg>, has_connections: bool,
                             sender_id: &str, data_type: &str) -> OdinServerResult<bool> {
        Ok(true) // TODO
    }

    // "setShared": { "key": "/incidents/czu/origin", "comment": "blah", "data": {"lat": 37.123, "lon": -122.12} }

    /// this is how we get data from clients. Called from ws input task of respective connection
    async fn handle_ws_msg (&mut self, 
        hself: &ActorHandle<SpaServerMsg>, remote_addr: &SocketAddr, ws_msg_parts: &WsMsgParts) -> OdinServerResult<WsMsgReaction> 
    {
        if ws_msg_parts.mod_path == ShareService::mod_path() {
            match ws_msg_parts.msg_type {
                "setShared" => {
                    match serde_json::from_str::<SetShared>(ws_msg_parts.payload) {
                        Ok(set_shared) => {
                            self.hstore.send_msg( SetSharedStoreEntry::from(set_shared)).await;
                        }
                        Err(e) => {
                            println!("SetShared payload failed to parse: {e}");
                        }
                    }
                }
                "removeShared" => {
                    if let Ok(remove_shared) = serde_json::from_str::<RemoveShared>( ws_msg_parts.payload) {
                        self.hstore.send_msg( RemoveSharedStoreEntry::from(remove_shared)).await;
                    }
                }
                "requestRole" => { // { "requestRole": "<new_role>" }
                    if let Ok(new_role) = serde_json::from_str::<String>( ws_msg_parts.payload) {
                        if !self.user_roles.contains_key(&new_role) {  // TODO - this could check authorization here
                            let role_entry = RoleEntry::new( *remote_addr);
                            let jv = role_entry.json_value( &new_role);

                            self.user_roles.insert( new_role.clone(), role_entry);

                            // notify owner of new role
                            let ws_msg = WsMsg::json(ShareService::mod_path(), "roleAccepted", jv.clone())?;
                            hself.send_msg( SendWsMsg{ remote_addr: *remote_addr, ws_msg}).await;

                            // notify all others
                            let ws_msg = WsMsg::json( ShareService::mod_path(), "extRoleAdded", jv)?;
                            hself.send_msg( SendAllOthersWsMsg{ except_addr: *remote_addr, ws_msg}).await;
                            
                        } else {
                            // TODO - should we give a reason here?
                            let ws_msg = WsMsg::json(ShareService::mod_path(), "roleRejected", new_role)?;
                            hself.send_msg( SendWsMsg{ remote_addr: *remote_addr, ws_msg}).await;
                        }
                    }
                }
                "releaseRoles" => {
                    if let Ok(roles) = serde_json::from_str::<Vec<String>>( ws_msg_parts.payload) {
                        let released_roles: Vec<String> = roles.iter().filter(|r| self.user_roles.contains_key(*r)).map(|r| r.clone()).collect();
                        if !released_roles.is_empty() {
                            for role in &released_roles {
                                self.user_roles.remove(role);
                            }

                            let ws_msg = WsMsg::json( ShareService::mod_path(), "rolesDropped", released_roles)?;
                            hself.send_msg( BroadcastWsMsg{ ws_msg}).await;
                        }
                    }
                }
                "startPublishRole" => {
                    if let Ok(role) = serde_json::from_str::<String>( ws_msg_parts.payload) {
                        if let Some(e) = self.user_roles.get_mut(&role) {
                            e.is_publishing = true;
                            let ws_msg = WsMsg::json( ShareService::mod_path(), "startPublish", role)?;
                            hself.send_msg( SendAllOthersWsMsg{except_addr: *remote_addr, ws_msg}).await;
                        }
                    }
                }
                "stopPublishRole" => {
                    if let Ok(role) = serde_json::from_str::<String>( ws_msg_parts.payload) {
                        if let Some(e) = self.user_roles.get_mut(&role) {
                            e.is_publishing = false;
                            let ws_msg = WsMsg::json( ShareService::mod_path(), "stopPublish", role)?;
                            hself.send_msg( SendAllOthersWsMsg{except_addr: *remote_addr, ws_msg}).await;
                        }
                    }
                }
                "publishCmd" => { // pass msg verbatim to all subscribers of the publishing role
                    if let Ok(publish_cmd) = serde_json::from_str::<PublishCmd>( ws_msg_parts.payload) {
                        if let Some(e) = self.user_roles.get(&publish_cmd.role) {
                            hself.send_msg( SendGroupWsMsg{ addr_group: e.subscribers.clone(), ws_msg: ws_msg_parts.ws_msg.to_string() }).await;
                        }
                    }
                }
                "publishMsg" => { // pass to all subscribers
                    if let Ok(publish_msg) = serde_json::from_str::<PublishMsg>( ws_msg_parts.payload) {
                        if let Some(e) = self.user_roles.get(&publish_msg.role) {
                            // TODO - we could log messages here
                            hself.send_msg( SendGroupWsMsg{ addr_group: e.subscribers.clone(), ws_msg: ws_msg_parts.ws_msg.to_string() }).await;
                        }     
                    }
                }
                "subscribeRole" => {
                    if let Ok(role) = serde_json::from_str::<String>( ws_msg_parts.payload) {
                        if let Some(e) = self.user_roles.get_mut(&role) {
                            if !e.subscribers.contains( remote_addr) {
                                e.subscribers.push( *remote_addr);
                                let ws_msg = WsMsg::json( ShareService::mod_path(), "updateRole", e.json_value(&role))?;
                                hself.send_msg( BroadcastWsMsg{ws_msg}).await;
                            }
                        }
                    }
                }
                "unsubscribeRole" => {
                    if let Ok(role) = serde_json::from_str::<String>( ws_msg_parts.payload) {
                        if let Some(e) = self.user_roles.get_mut(&role) {
                            if let Some(idx) = e.subscribers.iter().position(|rar| *rar == *remote_addr) {
                                e.subscribers.remove( idx);
                                let ws_msg = WsMsg::json( ShareService::mod_path(), "updateRole", e.json_value(&role))?;
                                hself.send_msg( BroadcastWsMsg{ws_msg}).await;
                            }
                        }
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

//--- the serde types that correspond to the websocket messages we receive (together with their SharedStoreActor message mapping) or send

#[derive(Debug, Serialize, Deserialize)]
pub struct SetShared {
    pub key: String,
    pub value: SharedItemType
}

impl From<SetShared> for SetSharedStoreEntry<SharedItemType> {
    fn from(ss: SetShared) -> Self {
        SetSharedStoreEntry{ key: ss.key, value: ss.value }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RemoveShared {
    pub key: String
}

impl From<RemoveShared> for RemoveSharedStoreEntry {
    fn from(rs: RemoveShared) -> Self {
        RemoveSharedStoreEntry{ key: rs.key }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PublishCmd {
    pub role: String,
    pub cmd: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PublishMsg {
    pub role: String,
    pub msg: String,
}

