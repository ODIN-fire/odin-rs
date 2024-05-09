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
#![allow(unused)]

/* #region data model ***********************************************************************************/
mod data {
    use odin_actor::{prelude::*, DynMsgReceiverList};
    use odin_actor::tokio_kanal::{Actor, AbortHandle,};
    use odin_server::PushWsMsgToAll;
    use std::fmt::Debug;
    use serde::{Serialize,Deserialize};

    #[derive(Serialize,Deserialize,Debug,Clone)]
    struct TrackUpdate {
        id: String,
        time: u64, // epoch millis
        lat: f64, // degrees
        lon: f64, // degrees
    }

    #[derive(Debug)] struct SubscribeToJsonUpdate(DynMsgReceiver<PushWsMsgToAll>);

    #[derive(Debug)] 
    struct ProcessJsonTracks <F,Fut> where F: FnOnce(String)->Fut, Fut: Future<Output=Result<()>> {
        action: F
    }
    impl <F,Fut> ProcessJsonTracks <F,Fut> where F: FnOnce(String)->Fut, Fut: Future<Output=Result<()>> {
        async fn process (self, s: String)->Result<()> {
            (self.action)(s).await
        }  
    }

    define_actor_msg_set! { TrackUpdaterMsg = SubscribeToJsonUpdate | ProcessJsonTracks<F,Fut> }
    
    /// this is just a simple data source actor we can subscribe to and not the focus of this example
    struct TrackUpdater {
        json_subscribers: DynMsgReceiverList<TrackUpdate>,
        timer: Option<AbortHandle>
    }

    impl TrackUpdater {
        fn update_tracks (&mut self) {

        }
    }

    impl_actor! { match msg for Actor<TrackUpdater,TrackUpdaterMsg> as
        _Start_ => cont! {
            self.timer = Some(self.hself.start_repeat_timer( 1, secs(5)));
        }
        _Timer_ => cont! {
            self.update_tracks();
        }
        SubscribeToJsonUpdate => cont! {
            self.json_subscribers.push( msg.0)
        }
        ProcessJsonTracks => cont! {
            todo!()
        }
    }
}

/* #endregion data model */

/* #region service **************************************************************************************/

/// this is the Service implementation example 
mod service {
    use std::os::unix::net::SocketAddr;

    use odin_actor::prelude::*;
    use axum::Router;
    use odin_server::{Service,Script,Stylesheet, PushWsMsgToConnection};

    struct TrackService {
        htrack_updater: MsgReceiver<SendCurrentTracksAsJson>
    }

    impl MicroService for TrackService {
        async fn send_init_ws_msg (&self, hserver: ActorHandle<ServerMsg>, remote_addr: SocketAddr)->Result<()> { 
            let hserver = hserver.clone();
            let action = |data| async move {
                hserver.send_msg( PushWsMsgToConnection{ remote_addr, data }).await
            };

            htrack_updater.send_msg( ProcessJsonTracks{action}).await?
        }
    }
}

/* #endregion service */

/* #region app *******************************************************************************************/

use tokio;
use anyhow::Result;
use odin_macros::define_service_type;

define_service_type!{ TrackServices = ImageryService + TrackService }

#[tokio::main]
async fn main()->Result<()> {
    let mut asys = ActorSystem::new("main");

    let htrack_updater = spawn_actor!( asys, "trackUpdater", TrackUpdater::new())?;

    let hserver = spawn_actor!( asys, "server", 
        Server<TrackServices>::new(
            app_name: "track_server",
            addr_spec: "http://localhost:3000",
            doc_path: "/tracks",
            services: TrackServices(
                ImageryService::new(),
                TrackService::new( h_track_updater)
            )
        ) 
    );

    htrack_updater.send_msg( SubscribeToJsonUpdate( hserver.into())).await?;

    asys.start_all(millis(20)).await?;
    asys.process_requests().await
}

/* #endregion app */