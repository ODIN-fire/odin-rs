/*
 * Copyright (c) 2024, United States Government, as represented by the
 * Administrator of the National Aeronautics and Space Administration.
 * All rights reserved.
 *
 * The ODIN - Open Data Integration Framework is licensed under the
 * Apache License, Version 2.0 (the "License"); you may not use this file
 * except in compliance with the License. You may obtain a copy of the
 * License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

#[macro_use]
extern crate lazy_static;

use std::{sync::Arc,path::PathBuf};
use odin_sentinel::actor::TriggerJsonSnapshot;
use tokio;
use structopt::StructOpt;
use odin_actor::prelude::*;
use odin_actor::tokio_kanal::{ActorSystem,Actor,ActorHandle};
use odin_config;
use odin_sentinel::{SentinelConfig,SentinelUpdate,actor::{SentinelConnector,SentinelConnectorMsg,AddInitCallback,AddUpdateCallback,AddJsonUpdateCallback}};
use anyhow::Result;


/* #region monitor actor *****************************************************************/

#[derive(Debug)] pub struct DataAvailable;

#[derive(Debug)] pub struct Snapshot(String);

#[derive(Debug)] pub struct Update(Arc<SentinelUpdate>);

#[derive(Debug)] pub struct JsonUpdate(Arc<String>);

define_actor_msg_type! { SentinelMonitorMsg = DataAvailable | Snapshot | Update | JsonUpdate }

struct SentinelMonitor {
    hconn: ActorHandle<SentinelConnectorMsg> 
}

impl_actor! { match msg for Actor<SentinelMonitor,SentinelMonitorMsg> as
    _Start_ => cont! {
        // we could also use an async_callback that directly sends the TriggerJsonSnapshot and then
        // the AddJsonUpdateCallback, which would save the SentinelDataAvailable. It's less readable
        // though because of the nesting (creating callbacks from within callback actions)
        let id = self.id().to_string();
        let hself = &self.hself;
        self.hconn.send_msg( AddInitCallback{id, action: msg_callback!(hself, DataAvailable)}).await.ok();
    }
    DataAvailable => cont! {
        let hself = &self.hself;
        let hconn = &self.hconn;
        hconn.send_msg( TriggerJsonSnapshot( msg_callback!( hself, |json:String| Snapshot(json)))).await.ok();
        hconn.send_msg( AddUpdateCallback {id: self.id().to_string(), action: msg_callback!( hself, |rec:Arc<SentinelUpdate>| Update(rec))}).await.ok();
        hconn.send_msg( AddJsonUpdateCallback {id: self.id().to_string(), action: msg_callback!( hself, |json:Arc<String>| JsonUpdate(json))}).await.ok();
    }
    Snapshot => cont! {
        println!("------------------------------ snapshot");
        println!("{}", msg.0);
    }
    Update => cont! { 
        println!("------------------------------ update");
        println!("{:?}", msg.0) 
    }
    JsonUpdate => cont! { 
        println!("------------------------------ JSON update");
        println!("JSON: {}", msg.0) 
    }
}

/* #endregion monitor actor */

#[derive(StructOpt)]
#[structopt(about = "example of how to use the SentinelConnector actor")]
struct CliOpts {
        /// path to sentinel config file
        config_path: PathBuf
}

lazy_static! {
    static ref ARGS: CliOpts = CliOpts::from_args();
}

#[tokio::main]
async fn main ()->Result<()> {
    //odin_config::init_from_os("gov", "nasa", Some("monitor_sentinels"))?;
    odin_config::init_from_project_root_dir("../config", "gov", "nasa-odin", Some("monitor_sentinels"))?;
    odin_config::ensure_dirs()?;

    let sentinel_config: SentinelConfig = odin_config::load_from_config_dir( "sentinel.ron")?;
    //println!("--- sentinel config:\n{:?}",sentinel_config);
    
    let mut actor_system = ActorSystem::new("main");

    let importer = spawn_actor!( actor_system, "importer", SentinelConnector::new(sentinel_config))?;
    let _ = spawn_actor!( actor_system, "monitor", SentinelMonitor{ hconn: importer })?;

    actor_system.timeout_start_all(millis(20)).await?;
    actor_system.process_requests().await?;

    Ok(())
}