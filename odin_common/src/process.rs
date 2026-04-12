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

use keepawake::KeepAwake;
use odin_build::get_bin_context;

/// execute the provided `exit_func` upon receiving a ctrl-c signal.
/// Note this does *not* automatically exit the process if not done so from `exit_func`.
pub fn set_ctrlc_handler<F> (mut exit_func: F)
    where F: FnMut()->() + Send + 'static
{
    ctrlc::set_handler( move || {
        exit_func();
    });
}

/// just an alias for std::process::exit()
#[inline] pub fn exit(exit_code: i32)-> ! { std::process::exit(exit_code) }

/// create an optional keep_awake object to prevent the machine from going to sleep
/// this is using the optional ODIN_KEEP_AWAKE environment variable, recognizing a comma separated list of "display", "idle" and "sleep" values
pub fn keep_awake()->Option<KeepAwake> {
    match std::env::var("ODIN_KEEP_AWAKE") {
        Ok(s) => {
            let vs = s.as_str().split(',').collect::<Vec<&str>>();
            let mut display: bool = vs.contains(&"display");
            let mut idle: bool = vs.contains(&"idle");
            let mut sleep: bool = vs.contains(&"sleep");

            if display || idle || sleep {
                let bin = if let Some(ctx) = get_bin_context() { ctx.bin_name.as_str() } else { "ODIN" };
                let reason = format!("ODIN_KEEP_ALIVE={}", s);

                match keepawake::Builder::default().app_name( bin).reason( &reason).display( display).idle( idle).sleep( sleep).create() {
                    Ok(keep_alive) => {
                        println!("{}", reason);
                        Some(keep_alive)
                    }
                    Err(e) => {
                        eprintln!("failed to create keep_alive object: {}", e);
                        None
                    }
                }

            } else {
                eprintln!("warning: unrecognized ODIN_KEEP_ALIVE ignored (possible comma separated values: 'display','idle','sleep') : {}", s);
                None
            }
        }
        _ => None
    }
}
