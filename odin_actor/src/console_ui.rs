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

use std::{sync::Arc,fmt};
use crate::{ActorSystemHandle, ActorSystemUITrait, DynActorSystemUI};

pub struct PingStatus {
    pub last_cycle: u32,
    pub last_ns: u64,
    pub min_ns: u64,
    pub max_ns: u64,
    pub avg_ns: u64,
    pub outlier: usize,

    pub was_outlier: bool, // state 
    // we could add variance here
}

impl PingStatus {
    pub fn new ()->Self { PingStatus{ last_cycle: 0, last_ns: 0, min_ns: 0, max_ns: 0, avg_ns: 0, outlier: 0, was_outlier: false } }

    fn update (&mut self, cycle: u32, last_ns: u64) {
        if cycle > 1 {
            if last_ns > 10* self.avg_ns && !self.was_outlier { // ignore one outlier
                //println!("@@ outlier: {}", last_ns);
                self.outlier += 1;
                self.was_outlier = true;

            } else {
                if last_ns < self.min_ns { self.min_ns = last_ns }
                if last_ns > self.max_ns { self.max_ns = last_ns }
        
                // we could round here but 1 nano_sec is already more resolution than realistic
                if last_ns > self.avg_ns {
                    self.avg_ns = self.avg_ns + (last_ns - self.avg_ns)/cycle as u64;
                } else {
                    self.avg_ns = self.avg_ns - (self.avg_ns - last_ns)/cycle as u64;
                }

                self.was_outlier = false;
            }
        } else {
            self.min_ns = last_ns; self.max_ns = last_ns; self.avg_ns = last_ns;
        }
        
        self.last_cycle = cycle;
        self.last_ns = last_ns;
    }
}

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
    pub fn new_boxed( hsys: Arc<ActorSystemHandle>)->Box<Self> { 
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

    fn heartbeat_cycle_started (&mut self, cycle: u32) {
        // print response for previous cycle
        if cycle > 1 {
            println!("-- heartbeat response: #{}", cycle -1);
            for (idx,e) in self.actor_entries.iter().enumerate() {
                println!("[{}]: {}", idx, e);
            }
        }
    }

    fn actor_heartbeat (&mut self, idx: usize, cycle: u32, last_ns: u64) {
        self.actor_entries[idx].status.update( cycle, last_ns);
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