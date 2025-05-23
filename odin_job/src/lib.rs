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

/// odin_job is a basic sdcheduler crate for sendable `FnMut` actions. Jobs can be scheduled
/// as oneshot or repeat, with a millisecond schedule resolution (which is more than most 
/// operating systems provide anyways).
/// The only exposed types are [`JobScheduler`] and [`JobHandle`]. Both are opaque.
///
/// Basic example: 
///```
///  use odin_job::JobScheduler;
///  ...
///  let mut scheduler = JobScheduler::new();
///  scheduler.run()?;
///  ...
///  scheduler.schedule_once( Duration::from_secs(4), println!("Hola!"));
///```  

use tokio::{self, select, spawn, task::{Builder,JoinHandle}, time::{sleep, Sleep}};
use kanal::{unbounded_async,AsyncReceiver,AsyncSender};
use std::{cmp::max, collections::VecDeque, fmt::Debug, sync::{atomic::{AtomicBool, AtomicU64, Ordering}, Arc, Mutex}, time::{Duration,SystemTime}};
use chrono::{DateTime, TimeZone};
use thiserror::Error;

#[derive(Error,Debug)]
pub enum OdinJobError {
    #[error("job queue not initialized")]
    NotInitialized,

    #[error("scheduler already running")]
    AlreadyRunning,

    #[error("max number of pending jobs exceeded")]
    MaxPendingJobs,

    #[error("spawn failed {0}")]
    SpawnFailed(String)
}

type Result<T> = std::result::Result<T,OdinJobError>;

pub struct JobContext {
    current_id: u64,
    cancel_repeat: bool,          // directive to cancel a repeat job
    reschedule: Option<Duration>  // directive to reschedule the current job
}
impl JobContext {
    pub fn current_id(&self)->u64 {
        self.current_id
    }
    pub fn cancel_repeat (&mut self) {
        self.cancel_repeat = true
    }

    pub fn reschedule_in (&mut self, dur: Duration) {
        self.reschedule = Some(dur)
    }
}

struct Job {
    id: u64,
    epoch_millis: u64,
    interval_millis: u64,
    action: Box<dyn FnMut(&mut JobContext) + Send>
}
impl Job {
    fn deadline (&self)->Sleep {
        let now_millis = now_epoch_millis();
        let wait_millis = if now_millis >= self.epoch_millis { 0 } else { self.epoch_millis - now_millis }; 
        sleep( Duration::from_millis( wait_millis))
    }

    fn execute (&mut self, ctx: &mut JobContext) {
        (self.action)(ctx);
    }
}
impl Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let abbrv_epoch = self.epoch_millis & 0x0000ffff;
        write!(f, "Job(id:{},epoch_millis:…{},interval_millis:{})", self.id, abbrv_epoch, self.interval_millis)
    }
}

#[derive(Debug)] 
pub struct JobHandle(u64);


pub struct JobScheduler {
    next_id: u64,
    queue: Arc<Mutex<VecDeque<Job>>>,
    max_pending: usize,
    tx: Option<AsyncSender<WakeUp>>,
    task: Option<JoinHandle<()>>
}

struct WakeUp{}

impl JobScheduler {
    pub fn new ()->Self {
        JobScheduler{ 
            next_id: 1, // note we start at id 1 (0 means no job)
            queue: Arc::new(Mutex::new(VecDeque::with_capacity(32))),
            max_pending: usize::MAX,
            tx: None, 
            task: None 
        }
    }

    pub fn with_max_pending (max_pending: usize)->Self {
        JobScheduler{ 
            next_id: 1, // note we start at id 1 (0 means no job)
            queue: Arc::new(Mutex::new(VecDeque::with_capacity(32))),
            max_pending,
            tx: None, 
            task: None 
        }
    }

    pub fn run (&mut self)->Result<()> {
        if self.task.is_none() {
            let (tx,rx) = kanal::unbounded_async::<WakeUp>();
            self.tx = Some(tx);

            let mut queue = self.queue.clone();
            self.task = Some( 
                Builder::new()
                    .name( "job-scheduler")
                    .spawn( async move {
                        loop {
                            let next_deadline: Option<Sleep> = {
                                let mut queue = queue.lock().unwrap();
                                queue.front().map(|job| job.deadline())
                            };

                            if let Some(deadline) = next_deadline {
                                tokio::pin!(deadline);

                                select! {
                                    _ = rx.recv() => {} // just a wakeup interrupt to schedule the next front()
                                    () = &mut deadline => {
                                        let mut queue = queue.lock().unwrap();
                                        if let Some(mut job) = queue.pop_front() {

                                            let mut ctx = JobContext { current_id: job.id, cancel_repeat: false, reschedule: None };
                                            job.execute(&mut ctx);

                                            if let Some(dur) = ctx.reschedule { // note this overrides a repeat cancellation
                                                job.epoch_millis += dur.as_millis() as u64;
                                                sort_in(job, &mut queue);
                                            } else {
                                                if job.interval_millis > 0 && !ctx.cancel_repeat {
                                                    // note we reschedule with the same id
                                                    job.epoch_millis += job.interval_millis;
                                                    sort_in(job, &mut queue);
                                                }
                                            }
                                        }
                                    }
                                }

                            } else { // queue is empty - wait for wakeup
                                rx.recv().await;
                            }
                        }
                    }
            ).map_err( |e| OdinJobError::SpawnFailed(e.to_string()))?);
            Ok(())

        } else {
            Err(OdinJobError::AlreadyRunning)
        }
    }

    pub fn is_running (&self)->bool { self.task.is_some() }

    pub fn schedule_once (&mut self, after: Duration, mut action: impl FnMut(&mut JobContext)+Send+'static)->Result<JobHandle> {
        self.schedule( after, None, action)
    }

    pub fn schedule_repeated (&mut self, after: Duration, interval: Duration, mut action: impl FnMut(&mut JobContext)+Send+'static)->Result<JobHandle> {
        self.schedule( after, Some(interval), action)
    }

    pub fn schedule_at<Tz: TimeZone> (&mut self, datetime: &DateTime<Tz>, mut action: impl FnMut(&mut JobContext)+Send+'static)->Result<JobHandle> {
        let now = now_epoch_millis();
        let dt = datetime.timestamp_millis();
        let after = if (dt < 0) || (dt as u64) < now { 0 } else { dt as u64 - now };

        self.schedule_once( Duration::from_millis(after), action)
    }

    pub fn schedule (&mut self, after: Duration, interval: Option<Duration>, mut action: impl FnMut(&mut JobContext)+Send+'static)->Result<JobHandle> {
        if let Some(tx) = &self.tx {
            let mut queue = self.queue.lock().unwrap(); // before we do anything acquire the queue lock

            let id = self.next_id;
            self.next_id += 1;

            if after.is_zero() {
                let mut ctx = JobContext { current_id: id, cancel_repeat: false, reschedule: None };
                action(&mut ctx);
                if interval.is_none() || ctx.cancel_repeat {
                    return Ok(JobHandle(id))
                }
            }

            if queue.len() < self.max_pending {
                let interval_millis = if let Some(interval) = interval { interval.as_millis() as u64 } else { 0 };
                let mut epoch_millis = now_epoch_millis() + after.as_millis() as u64;
                if after.is_zero() && interval_millis > 0 { epoch_millis += interval_millis }

                let job = Job { id, epoch_millis, interval_millis, action: Box::new(action) };
                // log job creation here

                if sort_in( job, &mut queue) == 0 { 
                    tx.try_send( WakeUp{});
                }

                Ok(JobHandle(id))
            } else {
                Err(OdinJobError::MaxPendingJobs)
            }

        } else {
            Err(OdinJobError::NotInitialized)
        }
    }

    pub fn is_pending_job (&self, jh: &JobHandle)->bool {
        let mut queue = self.queue.lock().unwrap();
        let id = jh.0;

        if id > 0 && id < self.next_id {
            for job in queue.iter() {
                if job.id == id { 
                    return true;
                }
            }
        }
        false
    }

    pub fn abort_job(&mut self, jh: JobHandle)->bool {
        let mut queue = self.queue.lock().unwrap();
        let id = jh.0;

        if id > 0 && id < self.next_id {
            for (idx,job) in queue.iter().enumerate() {
                if job.id == id { 
                    queue.remove(idx);
                    return true;
                }
            }
        }
        false
    }

    pub fn clear (&mut self) {
        let mut queue = self.queue.lock().unwrap();
        queue.clear();
    }

    // don't block here - this should be infallible
    pub fn abort (&mut self) {
        if let Some(task) = &self.task {
            task.abort(); // this will stop pending jobs from being executed
            self.tx = None;
            self.next_id = 1;
            self.task = None;
        }
    }
}

// ensure this is only called after acquiring the queue lock
fn sort_in (job: Job, queue: &mut VecDeque<Job>)->usize {
    if queue.is_empty() {
        queue.push_front(job);
        0

    } else { // since queue is not empty we can safely unwrap front()/back()
        if job.epoch_millis >= queue.back().unwrap().epoch_millis { // new back
            queue.push_back(job);
            queue.len()-1

        } else if job.epoch_millis >= queue.front().unwrap().epoch_millis { // somewhere in between
            for (idx,j) in queue.iter().enumerate() {
                if job.epoch_millis < queue[idx].epoch_millis {
                    queue.insert(idx, job);
                    return idx
                }
            }
            queue.len()-1 // can't happen

        } else { // new front 
            queue.push_front(job);
            0
        }
    }
}

#[inline]
fn now_epoch_millis()->u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis() as u64
} 
