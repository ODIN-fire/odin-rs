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
use crate::{N5Config,N5Connector,errors::Result,actor::N5ActorMsg};

/// an http based N5Connector
/// Note that `N5Connector` instances are used for dependency injection into [`N5Actor`] and hence
/// are created before we have a respective [`ActorHandle`]. This means the purpose of a `LiveN5Connector` is
/// twofold: 
///   - (a) to provide an [`N5Connector`] impl that is used by the actor, and 
///   - (b) to create the internal `LiveConnection` object that does the real work once the actor calls 
///    `N5Connector::start(actor_handle)` (during processing of its _Start_ message).
/// 
/// Note also that LiveN5Connector is a configured object. Since it has to pass down the config into
/// its LiveConnection it keeps the config in an `Arc`
/// 
pub struct LiveN5Connector { 
    config: Arc<N5Config>,
    connection: Option<LiveConnection>
}

impl LiveN5Connector {
    /// called before actor instantiation
    pub fn new (config: N5Config)->Self {
        LiveN5Connector { config: Arc::new(config), connection: None }
    }

    /// called from actor _Start_ (2nd half of our initialization)
    async fn initialize (&mut self, hself: ActorHandle<N5ActorMsg>)->Result<()> {
        self.connection = Some(LiveConnection::new(self.config.clone(), hself).await?);
        Ok(())
    }
}

/// this is the interface used by the [`N5Actor`] 
#[async_trait]
impl N5Connector for LiveN5Connector {
    async fn start (&mut self, hself: ActorHandle<N5ActorMsg>)->Result<()> {
        self.initialize(hself).await
    }

    fn terminate (&mut self) {
        if let Some(mut conn) = self.connection.as_mut() {
            conn.terminate();
            self.connection = None;
        }
    }
}

/// the (internal) worker of a LiveN5Connector (dynamically created)
struct LiveConnection {
    config: Arc<N5Config>,
}

impl LiveConnection {
    async fn new (config: Arc<N5Config>, hself: ActorHandle<N5ActorMsg>)->Result<Self> {
        Ok( LiveConnection{config} )
    }

    fn terminate(&mut self) {
    }
}