/*
 * Copyright 2026 Peter Mehlitz
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *    http://apache.org
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

//! run_scrubber - run the odin_scrub file tree scrubber loop as a separate process, or check what a config would
//! delete with `--dry-run`

#![allow(unused)]

use std::{path::{Path,PathBuf}};
use anyhow::{anyhow,Result};
use tracing_subscriber::EnvFilter;
use odin_common::{define_cli};
use odin_scrub::{ScrubConfig, load_config, scrub_once, spawn_scrubber};

define_cli! { ARGS [about="run_scrubber - periodically delete aged files and directories according to a ScrubConfig"] =
    dry_run: bool [help="only show what would be deleted in a single scrub pass, without deleting anything", long],
    config: String [help="ScrubConfig file path or name of odin_scrub config (looked up through odin_build)", default_value="scrub.ron"]
}

fn main ()->Result<()> {
    odin_build::set_bin_context!();
    tracing_subscriber::fmt().with_env_filter( EnvFilter::try_from_default_env().unwrap_or_else( |_| EnvFilter::new("info"))).init();

    let config = get_config( &ARGS.config)?;

    if ARGS.dry_run {
        println!("dry run for root dir {:?}", config.root_dir);
        let stats = scrub_once( &config, true)?;
        println!("would have {}", stats.summary( config.show_stats));
        Ok(())

    } else {
        // the scrubber thread only terminates if the loop fails
        spawn_scrubber( config)?.join().map_err( |_| anyhow!("scrubber thread panicked"))?
    }
}

/// if the argument is an existing file we use it directly, otherwise we look it up as an odin_scrub config
fn get_config (config: &str)->Result<ScrubConfig> {
    let path = Path::new( config);
    if path.is_file() {
        let src = std::fs::read_to_string( path)?;
        Ok( ron::from_str( &src).map_err( |e| anyhow!("invalid config {path:?}: {e}"))?)
    } else {
        Ok( load_config( config)?)
    }
}
