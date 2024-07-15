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

use std::env;
use std::process::Command;

/// tool to set required ODIN_... env vars when building odin bins with embedded resources (configs or assets)
/// usage: ```build_odin_bin [--embed] [--root ❬dir❭] [--reload] [❬cargo-opts❭...] ❬bin-name❭```
/// This runs ```cargo build [❬cargo-opts❭...] --bin ❬bin-name❭``` with respective ODIN_... env vars
/// 
/// The following ODIN_.. environment variables are set:
/// 
/// - ODIN_BIN_CRATE - name of the crate for the bin target to build
/// - ODIN_BIN_NAME - name of the bin target
/// - ODIN_HOME - name of the ODIN root directory
/// - ODIN_EMBED_RESOURCES - build bin target with embedded resources
/// - ODIN_RELOAD_ASSETS - build bin target without file asset caching (dev mode for scripts)
/// 
/// note that ODIN_EMBEDDED_ONLY and ODIN_RELOAD_ASSETS are runtime-checked environment variables (not used at build-time)
fn main () {
    if env::args().len() < 2 {
        println!("bob - cargo build wrapper to build ODIN binaries with embedded resources");
        println!("bob [--embed] [--root ❬dir❭] [❬cargo-opts❭...] ❬bin-name❭");
        println!("  --embed      : build binary with embedded resources");
        println!("  --root ❬dir❭ : set ODIN root dir to embed resources from");
        return;
    }

    let cur_dir = env::current_dir().unwrap();
    let bin_crate = cur_dir.file_name().unwrap().to_str().unwrap(); // how could there not be a current dir
    let bin_name = env::args().last().expect("no binary name given");

    let mut cargo = Command::new("cargo");

    cargo
        .env( "ODIN_BIN_CRATE", bin_crate)
        .env( "ODIN_BIN_NAME", bin_name.as_str())
        .arg( "build");

    let args: Vec<String> = env::args().collect();
    let n = args.len()-1;
    let mut embed_resources = "false";
    let mut build_type = "debug";
    let mut features = String::new(); 

    let mut i = 1;
    while i < n {
        let a = &args[i];
        if a == "--embed" {
            embed_resources = "true";
            push_feature( &mut features, "embedded_resources");

        } else if a == "--root" { // only used if embed is set. Takes a dir value
            if i < n-1 {
                i += 1;
                let dir = &args[i];
                cargo.env("ODIN_HOME", dir);
            }

        } else if a == "--features" {
            if i < n-1 {
                i += 1;
                push_feature( &mut features, &args[i].as_str());
            }


        } else if a != "run" && a != "build" {
            if a == "--release" { build_type = "release"; }
            cargo.arg(a);
        }

        i += 1;
    }

    cargo.env("ODIN_EMBED_RESOURCES", embed_resources);

    cargo
        .arg("--bin")
        .arg( bin_name.as_str());

    if !features.is_empty() {
        cargo.arg( "--features");
        cargo.arg( features);
    }

    println!("executing {:?}", cargo);

    if let Ok(mut child) = cargo.spawn() {
        match child.wait() {
            Ok(exit_status) => {
                if exit_status.success() {
                    println!("\nbuilt odin binary ../target/{build_type}/{bin_name}");
                } else {
                    eprintln!("cargo failed with {exit_status}");
                }
            }
            Err(e) => eprintln!("waiting for cargo completion failed with {e:?}")
        }
    } else {
        eprintln!("cargo didn't start");
    }
} 

fn push_feature (features: &mut String, new_feature: &str) {
    if !features.is_empty() { features.push(','); }
    features.push_str( new_feature);
}