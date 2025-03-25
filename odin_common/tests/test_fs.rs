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

use odin_common::fs::matching_files_in_dir;
use regex::Regex;
use std::path::Path;

// run with "cargo test test_xx -- --nocapture"

#[test]
fn test_matching_files() {
    let re = Regex::new( r".*\.rs").unwrap();
    let dir = Path::new("src");
    let res = matching_files_in_dir( &dir, &re);

    assert!(res.is_ok());

    if let Ok(files) = res {
        assert!( !files.is_empty());
        for f in files {
            println!("{f:?}");
        }
    } else {
        panic!("no matching files in src/ ?")
    }
}