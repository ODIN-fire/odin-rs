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
#![allow(unused)]

use std::{path::Path, fs::remove_file};
use anyhow::{anyhow,Result};
use odin_common::define_cli;
use odin_image::{get_hex_rgb,overlay_imgs};


define_cli! { ARGS [about="overlay images of same size"] =
    files: Vec<String> [help="<input file>... <output-file>"]
}

fn main() -> Result<()> {
    let n_files = ARGS.files.len();
    if n_files <=2 {
        return Err( anyhow!("not enough files (needs at least 2 inputs and one output)"))
    }

    let out_fname = Path::new( &ARGS.files[n_files-1]);
    if out_fname.is_file() {
        remove_file( out_fname)?;
    }

    let in_fnames: Vec<&str> = ARGS.files[0..n_files-1].iter().map( |p| p.as_str()).collect();
    let out_fname = ARGS.files[n_files-1].as_str();

    overlay_imgs( &in_fnames, out_fname)?;

    Ok(())
}
