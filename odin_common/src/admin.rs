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

use std::{panic,ops::Drop,thread::sleep,time::Duration};
use chrono;
use lazy_static::lazy_static;
use serde::Deserialize;
use ctrlc;

use odin_build::get_bin_context;
use crate::fs::filename_of_path;

#[cfg(feature="slack_admin")]
use crate::{load_config,slack::blocking_send_msg,slack::send_msg};

#[derive(Clone,Copy)]
enum Severity {
    Critical,
    Severe,
    Info
}

impl Severity {
    fn name (&self)->&'static str {
        match self {
            Severity::Critical => "critical",
            Severity::Severe   => "severe",
            Severity::Info     => "info"
        }
    }

    fn icon (&self)->&'static str {
        match self {
            Severity::Critical => ":red_circle:",
            Severity::Severe => ":large_orange_circle:",
            Severity::Info => ":large_green_circle:"
        }
    }
}

/// collection sysadmin functions to be used in ODIN applications

#[cfg(feature="slack_admin")]
#[derive(Deserialize,Debug)]
struct SlackAdminConfig {
    channel_id: String,
    token: String,
}

pub struct ExitGuard {}

impl Drop for ExitGuard {
    fn drop(&mut self) {
        notify_info("terminated normally");
    }
}

#[cfg(feature="slack_admin")]
lazy_static! {
    static ref SLACK_CONFIG: SlackAdminConfig = load_config("slack_admin.ron").unwrap(); // we want to panic here
}

/// this macro needs to be called in main() at the top level as it uses the current scope to detect program
/// termination (both nominal and abnormal)
#[macro_export]
macro_rules! monitor_executable {
    () => {
        odin_common::admin::initialize();
        odin_common::admin::notify_info("started");
        let _exit_guard_ = odin_common::admin::ExitGuard{}; // to make sure we get a corresponding terminated notification
    }
}
pub use monitor_executable;

pub fn initialize () {
    #[cfg(feature="slack_admin")]
    let _ = &SLACK_CONFIG.channel_id;  // make sure SLACK_CONFIG is initialized (panic if not)

    std::panic::set_hook( Box::new( |panic_info| {
        let msg = panic_info.to_string();
        notify_critical( &msg);
    }));
}

fn notify (severity: Severity, msg: &str) {
    notify_console( severity, msg);

    #[cfg(feature="slack_admin")]
    notify_slack( severity, msg);
}

fn notify_console (severity: Severity, msg: &str) {
    match severity {
        Severity::Critical => eprint!("\x1b[31;40m"), // red on black
        Severity::Severe   => eprint!("\x1b[93;40m"), // yellow on black
        Severity::Info     => eprint!("\x1b[97;40m"), // white on black
    }
    eprintln!("{} ADMIN [{}] {}\x1b[0m", chrono::Local::now().format("%d/%m/%Y %H:%M:%S"), severity.name(), msg);
}

#[cfg(feature="slack_admin")]
fn notify_slack (severity: Severity, msg: &str) {
    std::thread::sleep( Duration::from_secs(1)); // make sure we don't run into Slack chat msg rate limits

    let txt = format!("{}[{}]: {} {}\n>{}", severity.icon(), severity.name(), chrono::Local::now().format("%d/%m/%Y %H:%M:%S"), bin_name(), msg);
    blocking_send_msg( &SLACK_CONFIG.token, &SLACK_CONFIG.channel_id, &txt, None);
}

async fn async_notify (severity: Severity, msg: &str) {
    notify_console( severity, msg);

    #[cfg(feature="slack_admin")]
    async_notify_slack( severity, msg).await;
}

#[cfg(feature="slack_admin")]
async fn async_notify_slack (severity: Severity, msg: &str) {
    use tokio;
    tokio::time::sleep( Duration::from_secs(1)).await; // make sure we don't run into Slack chat msg rate limits

    let txt = format!("{}[{}]: {} {}\n>{}", severity.icon(), severity.name(), chrono::Local::now().format("%d/%m/%Y %H:%M:%S"), bin_name(), msg);
    send_msg( &SLACK_CONFIG.token, &SLACK_CONFIG.channel_id, &txt, None).await;
}

fn bin_name()-> String {
    if let Some(ctx) = get_bin_context() {
        ctx.bin_name.clone()
    } else {
        if let Ok(path) = std::env::current_exe() {
            filename_of_path(path).unwrap_or( "?".to_string())
        } else { "?".to_string() }
    }
}

//--- this is the public api for explicitly sent messages

#[inline] pub fn notify_critical (msg: &str) { notify( Severity::Critical, msg) }
#[inline] pub fn notify_severe (msg: &str) { notify( Severity::Severe, msg) }
#[inline] pub fn notify_info (msg: &str) { notify( Severity::Info, msg) }

//--- async versions

#[inline] pub async fn async_notify_critical (msg: &str) { async_notify( Severity::Critical, msg).await }
#[inline] pub async fn async_notify_severe (msg: &str) { async_notify( Severity::Severe, msg).await }
#[inline] pub async fn async_notify_info (msg: &str) { async_notify( Severity::Info, msg).await }