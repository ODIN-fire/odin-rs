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

use std::{thread::sleep,time::Duration};
use odin_common::{admin,process};


fn main() {
    admin::monitor_executable!();
    process::set_ctrlc_handler(|| {
        admin::notify_info("terminated by signal");
        process::exit(0);
    });
    delay_secs( 1); // make sure we don't exceed Slack chat message rate-limits (there is also built-in delay in admin::notify)

    admin::notify_info("this is harmless");
    delay_secs( 1);

    admin::notify_severe("this isn't harmless");
    delay_secs( 1);

    admin::notify_critical("the sky is about to fall!");
    delay_secs( 1);

    //panic!("something went awfully wrong");

    delay_secs( 10); // try Ctrl-C
}

fn delay_secs (secs: u64) {
    sleep(Duration::from_secs( secs));
}