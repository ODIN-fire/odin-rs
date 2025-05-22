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

use std::{sync::Arc,path::{Path,PathBuf},time::Duration,collections::HashSet};
use chrono::{DateTime,Utc,Timelike};

use odin_actor::prelude::*;
use odin_actor::{error,debug,warn,info};
use odin_common::{datetime::{full_hour,hours}, fs::{remove_old_files, FileAvailable}};

use crate::{errors::*, get_next_base_step, is_extended_forecast, queue_available_forecasts, DownloadCmd, HrrrConfig, HrrrDataSetConfig, HrrrDataSetRequest, HrrrFileAvailable, HrrrFileRequest};
use crate::{spawn_download_task, hrrr_cache_dir, schedule::{HrrrSchedules, get_statistic_schedules}};

#[derive(Debug)]
pub struct AddDataSet (pub Arc<HrrrDataSetRequest>);

#[derive(Debug)]
pub struct RemoveDataSet (pub Arc<HrrrDataSetRequest>);


define_actor_msg_set! { pub HrrrActorMsg = AddDataSet | RemoveDataSet }

/// the state of an actor that periodically retrieves HRRR files and executes a configured
/// action for each of the downloaded files
pub struct HrrrActor {
    config: Arc<HrrrConfig>,
    datasets: HashSet<Arc<HrrrDataSetRequest>>,

    tx: MpscSender<DownloadCmd>,
    download_task: JoinHandle<()>,

    base: DateTime<Utc>,
    step: usize,

    // all set during start
    schedules: HrrrSchedules,
    timer: Option<AbortHandle>,
}

impl HrrrActor {
    pub fn new <A> (config: HrrrConfig, schedules: HrrrSchedules, file_avail_action: A)->Self 
        where A: DataAction<HrrrFileAvailable> + 'static
    {
        let config = Arc::new(config);
        let cache_dir = hrrr_cache_dir();
        let (download_task,tx) = spawn_download_task( config.clone(), cache_dir, file_avail_action).unwrap();

        HrrrActor {
            config,
            datasets: HashSet::new(),

            tx,
            download_task,

            base: Utc::now(), // reset upon start
            step: 0,

            schedules,
            timer: None
        }
    }

    async fn add_dataset (&mut self, ds: Arc<HrrrDataSetRequest>) {
        if !self.datasets.contains( &ds) {
            queue_available_forecasts( &self.tx, ds.clone(), &self.schedules).await;

            self.datasets.insert( ds);

            if self.datasets.len() == 1 {
                self.set_base_step();
            }
        }
    }

    fn set_base_step (&mut self) {
        let now = Utc::now();
        let (base,step) = get_next_base_step( &self.schedules, &now);

        //println!("@@ start at {} + {}", base, step);

        self.base = base;
        self.step = step;
    }

    async fn check_step (&mut self) {
        if !self.datasets.is_empty() {
            let now = Utc::now();
            let mut sched = self.schedules.schedule_for(&self.base);

            while (now - self.base).num_minutes() as u32 >= sched[self.step] {
                for ds in &self.datasets {
                    let cmd = DownloadCmd::GetFile( HrrrFileRequest {ds: ds.clone(), base: self.base, step: self.step} );
                    self.tx.send( cmd).await;
                }
                self.step += 1;

                if self.step >= sched.len() { // next cycle
                    self.base = self.base + hours(1);
                    self.step = 0;
                    sched =  self.schedules.schedule_for(&self.base);
                }
            }
        }
    }

    fn remove_dataset (&mut self, ds: Arc<HrrrDataSetRequest>) {
        self.datasets.remove(&ds);
    }

    fn terminate (&mut self) {
        self.download_task.abort();
        if let Some(timer) = &self.timer { timer.abort() }
    }
}


impl_actor! { match msg for Actor<HrrrActor,HrrrActorMsg> as
    AddDataSet => cont! {
        self.add_dataset(msg.0).await;

        if self.datasets.len() == 1 { // first request, start timer
            if let Ok(timer) = self.start_repeat_timer( 1, self.config.check_interval, false) {
                self.timer = Some(timer);
            } else { error!("failed to start timer") }
        }
    }
    RemoveDataSet => cont! {
        self.remove_dataset(msg.0);

        if self.datasets.is_empty() { // last request, stop timer
            if let Some(timer) = &self.timer {
                timer.abort();
                self.timer = None;
            }
        }
    }
    _Timer_ => cont! { 
        self.check_step().await
    }
    _Terminate_ => stop! { 
        self.terminate();
    }
} 