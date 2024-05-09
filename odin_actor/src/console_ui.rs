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

use std::{sync::Arc,fmt};
use crate::{ActorSystemHandle, ActorSystemUITrait, DynActorSystemUI, PingStatus};

/// example struct for display relevant actor data
struct ActorDisplayData {
    id: Arc<String>,
    type_name: &'static str,
    status: PingStatus
}

impl fmt::Display for ActorDisplayData {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let status = &self.status;
        write!(f, "{:<15}: {:>6} ns,  avg: {:>5}, min: {:>5}, max: {:>6}, outlier: {:>2}", 
                   self.id, status.last_ns, status.avg_ns, status.min_ns, status.max_ns, status.outlier)
    }
}

/// a simple ActorSystemUITrait implementation that just prints to stdout. 
/// This is just a logger that serves as an example of how to use the ActorSystemUITrait api
/// to manage UI display data
pub struct ConsoleUI {
    hsys: Arc<ActorSystemHandle>, // in case this is an interactive UI that wants to send ActorSystemRequests
    actor_entries: Vec<ActorDisplayData>
}

impl ConsoleUI {
    pub fn boxed( hsys: Arc<ActorSystemHandle>)->Box<Self> { 
        Box::new( ConsoleUI { hsys, actor_entries: Vec::new() } ) 
    }
}

impl ActorSystemUITrait for ConsoleUI {
    fn actors_started (&mut self) {
        println!("-- actors started");
    }

    fn add_actor (&mut self, id: Arc<String>, type_name: &'static str) {
        println!("-- actor added: {}: {}", id, type_name);
        self.actor_entries.push( ActorDisplayData { id, type_name, status: PingStatus::new() });
    }

    fn remove_actor (&mut self, idx: usize) {
        println!("-- actor removed: {}", self.actor_entries[idx]);
        self.actor_entries.remove(idx);
    }

    fn no_start_actor (&mut self, idx: usize) {
        println!("-- actor did not start: {}", self.actor_entries[idx]);
    }

    fn heartbeats_started (&mut self) {
        println!("-- heartbeats started");
    }

    fn heartbeat (&mut self, cycle: u32) {
        println!("-- heartbeat: {}", cycle);
    }

    fn heartbeat_response (&mut self, cycle: u32, entries: Vec<PingStatus>) {
        println!("-- heartbeat response: {}", cycle);
        for (idx,s) in entries.iter().enumerate() {
            self.actor_entries[idx].status = *s;
            println!("{}", self.actor_entries[idx]);
        }
    }

    fn unresponsive_actor (&mut self, idx: usize) {
        println!("-- actor unresponsive: {}", self.actor_entries[idx]);
    }

    fn no_terminate_actor (&mut self, idx: usize) {
        println!("-- actor did not terminate: {}", self.actor_entries[idx]);
    }

    fn actors_terminated (&mut self) {
        println!("-- actors terminated");
    }
}