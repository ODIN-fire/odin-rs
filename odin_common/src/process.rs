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