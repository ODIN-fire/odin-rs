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

 use std::{panic,ops::Drop};
 use chrono;
 use lazy_static::lazy_static;
 use serde::Deserialize;

 #[cfg(feature="slack_admin")]
 use crate::{load_config,slack::blocking_send_msg};

#[cfg(feature="slack_admin")]
use odin_build::get_bin_context;


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
        notify_info("terminated");
    }
}

#[cfg(feature="slack_admin")]
lazy_static! {
    static ref SLACK_CONFIG: SlackAdminConfig = load_config("slack_admin.ron").unwrap(); // we want to panic here
}

/// this macro needs to be called in main() at the top level
#[macro_export]
macro_rules! monitor_executable {
    () => {
        odin_common::admin::initialize();

        std::panic::set_hook( Box::new( |panic_info| {
            let msg = panic_info.to_string();
            odin_common::admin::notify_critical( &msg);
        }));

        odin_common::admin::notify_info("started");
        let _exit_guard_ = odin_common::admin::ExitGuard{}; // to make sure we get a corresponding terminated notification
    }
}

pub fn initialize () {
    #[cfg(feature="slack_admin")]
    let _ = &SLACK_CONFIG.channel_id;  // make sure SLACK_CONFIG is initialized (panic if not)
}

/// console fallback
#[cfg(not(feature="slack_admin"))]
fn notify (severity: &str, msg: &str) {
    notify_console( severity, msg);
}

/// send notification to slack
#[cfg(feature="slack_admin")]
fn notify (severity: &str, msg: &str) {
    use std::env;

    use crate::fs::filename_of_path;

    notify_console( severity, msg);

    let bin_name = if let Some(ctx) = get_bin_context() {
        ctx.bin_name.clone()
    } else {
        if let Ok(path) = env::current_exe() {
            if let Ok(s) = filename_of_path(path) { s } else { "?".to_string() }
        } else { "?".to_string() }
    };
    let txt = format!("[{}]: {} {}\n{}", severity.to_uppercase(), chrono::Local::now().format("%d/%m/%Y %H:%M:%S"), bin_name, msg);
    blocking_send_msg( &SLACK_CONFIG.token, &SLACK_CONFIG.channel_id, &txt);
}

fn notify_console (severity: &str, msg: &str) {
    match severity {
        "critical" => eprint!("\x1b[31;40m"),
        "severe" => eprint!("\x1b[93;40m"),
        "info" => eprint!("\x1b[97;40m"),
        &_ => {}
    }
    eprintln!("{} ADMIN [{}] {}\x1b[0m", chrono::Local::now().format("%d/%m/%Y %H:%M:%S"), severity, msg);
}

#[inline] pub fn notify_critical (msg: &str) { notify( "critical", msg) }
#[inline] pub fn notify_severe (msg: &str) { notify( "severe", msg) }
#[inline] pub fn notify_info (msg: &str) { notify( "info", msg) }