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

//! example application of how to create and use a HimawariHotspotActor in a standalone, configured executable.

use std::sync::Arc;
use chrono::{DateTime, Utc, Datelike, Timelike};
use anyhow::Result;
use odin_himawari::{HimawariConfig, PKG_CACHE_DIR};
use odin_actor::prelude::*;
use odin_himawari::actor::{self, HimawariHotspotActor};
use odin_himawari::{live_importer::LiveHimawariHotspotImporter, HimawariHotspotStore, HimawariHotspotSet, load_config};
use odin_build;

//--- example client actor
#[derive(Debug)] pub struct Init(String);
#[derive(Debug)] pub struct Update(String);

define_actor_msg_set! { HimawariMonitorMsg = Init | Update }
struct HimawariMonitor {}

impl_actor! { match msg for Actor<HimawariMonitor,HimawariMonitorMsg> as
    Init => cont! {
        println!("------------------------------ init {}", Utc::now().time());
        println!("{}", msg.0)
    }
    Update => cont! {
        println!("------------------------------ update {}", Utc::now().time());
        println!("{}", msg.0)
    }
}

//--- set up actor system
run_actor_system!( actor_system => {
    let hmonitor = spawn_actor!( actor_system, "monitor", HimawariMonitor{})?;

    let config = Arc::new(odin_himawari::load_config::<HimawariConfig>("himawari.ron")?);
    let _hactor = spawn_actor!( actor_system, "himawari", HimawariHotspotActor::new(
        config.clone(),
        LiveHimawariHotspotImporter::new(config, Arc::new(PKG_CACHE_DIR.clone())),
        dataref_action!( let hmonitor: ActorHandle<HimawariMonitorMsg> = hmonitor.clone() => |store: &HimawariHotspotStore| {
            for hs in store.iter_old_to_new(){
                let msg = Init(hs.to_json_pretty().unwrap());
                hmonitor.try_send_msg(msg);
            }
            Ok(())
        }),
        data_action!( let hmonitor: ActorHandle<HimawariMonitorMsg> = hmonitor => |hs:HimawariHotspotSet| {
            let msg = Update(hs.to_json_pretty().unwrap());
            Ok( hmonitor.try_send_msg( msg)? )
        }),
    ))?;

    Ok(())
});
