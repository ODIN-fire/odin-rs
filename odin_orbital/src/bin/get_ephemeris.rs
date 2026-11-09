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

//! get_ephemeris - explicitly retrieve or update the satkit data files (JPL ephemeris, gravity models, earth
//! orientation parameters and space weather) used by odin_orbital.
//!
//! By default files are stored in `ODIN_ROOT/data/odin_orbital/satkit`. Static files (ephemeris, gravity models)
//! are only downloaded if they are missing, regularly updated files (EOP, space weather) are refreshed according to
//! their publication cadence. Use `--force` to re-fetch all files.
//!
//! Run with `ODIN_SKIP_SATKIT_INIT=1` to suppress the automatic (conditional) update during odin_orbital initialization

use std::path::PathBuf;
use anyhow::{anyhow,Result};
use odin_common::{define_cli, fs::ensure_writable_dir};
use odin_orbital::satkit_data_dir;

define_cli! { ARGS [about="retrieve or update satkit ephemeris and data files used by odin_orbital"] =
    force: bool [help="re-fetch all files even if they are present and up-to-date", long],
    dir: Option<String> [help="alternative target directory (default is ODIN_ROOT/data/odin_orbital/satkit)", long]
}

fn main ()->Result<()> {
    odin_build::set_bin_context!();

    let dir = if let Some(dir) = &ARGS.dir { PathBuf::from(dir) } else { satkit_data_dir() };
    ensure_writable_dir( &dir)?;

    satkit::utils::update_datafiles( Some(dir.clone()), ARGS.force)
        .map_err( |e| anyhow!("failed to update satkit data files in {dir:?}: {e}"))?;

    // record the update so that odin_orbital initialization does not update again within the next day
    odin_common::fs::set_modified_timestamp( &dir, std::time::SystemTime::now())?;
    println!("satkit data files in {dir:?} updated");

    Ok(())
}
