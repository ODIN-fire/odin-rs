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

use std::sync::Arc;
use async_trait::async_trait;
use odin_actor::prelude::*;
use reqwest::Client;
use crate::{
    N5Config,N5Connector,errors::Result,actor::{N5ActorMsg,InitStore,UpdateStore}, 
    get_n5_devices, get_n5_data
};

/// an http based N5Connector
/// Note that `N5Connector` instances are used for dependency injection into [crate::actor::N5Actor] and hence
/// are created before we have a respective [`ActorHandle`]
pub struct LiveN5Connector { 
    config: Arc<N5Config>,
    task: Option<AbortHandle>
}

impl LiveN5Connector {
    /// called before actor instantiation
    pub fn new (config: N5Config)->Self {
        LiveN5Connector { config: Arc::new(config), task: None }
    }
}

/// this is the interface used by the N5Actor
#[async_trait]
impl N5Connector for LiveN5Connector {

    async fn start (&mut self, hself: ActorHandle<N5ActorMsg>)->Result<()> {
        if self.task.is_none() {
            let config = self.config.clone();

            let jh = spawn( "N5-connector", async move {
                let client = Client::new();
                if let Ok(devices) = get_n5_devices( &client, config.as_ref(), true).await {
                    let device_ids: Vec<u32> = devices.iter().map( |d| d.id).collect();
                    hself.send_msg( InitStore(devices)).await;

                    loop {
                        sleep( config.retrieve_interval).await;
                        if let Ok(updates) = get_n5_data( &client, config.as_ref(), &device_ids).await {
                            hself.send_msg( UpdateStore(updates)).await;
                        }
                    }
                }
            })?;
            self.task = Some(jh.abort_handle());
        }
        Ok(())
    }

    fn terminate (&mut self) {
        if let Some(ah) = &self.task {
            ah.abort();
            self.task = None;
        }
    }
}
