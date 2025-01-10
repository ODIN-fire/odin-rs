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
#![allow(unused)]

use std::{path::{Path,PathBuf}, fs::{read_dir,copy}};
use odin_common:: {
    define_cli, 
    fs::{self, ensure_writable_dir, existing_non_empty_file_from_path}
};
use globset::{Glob,GlobSet,GlobSetBuilder};
use anyhow::{Result,anyhow};

define_cli! { ARGS [about="duplicate_dir - duplicate directory tree"] =
    link_files: bool [ help="only use symbolic (soft) links for files", long, short],
    exclude: Vec<String> [help="exclude file or directory matching glob", long, short, number_of_values=1],
    source_dir: String [help="root directory to duplicate"],
    target_dir: String [help="directory to duplicate to (will be created/overwritten)"]
}

fn main()->Result<()> {
    let excludes: GlobSet = get_excludes()?;
    let src_dir = Path::new(&ARGS.source_dir).to_path_buf();
    if !src_dir.is_dir() { Err( anyhow!("source dir does not exist: {:?}", src_dir))? }

    let tgt_dir = Path::new(&ARGS.target_dir).to_path_buf();
    ensure_writable_dir(&tgt_dir)?;

    duplicate_dir( &src_dir, &tgt_dir, &excludes);

    Ok(())
}

fn get_excludes()->Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for p in &ARGS.exclude {
        builder.add( Glob::new(p)?);
    }
    Ok( builder.build()? )
}

fn duplicate_dir (src_dir: &PathBuf, tgt_dir: &PathBuf, excludes: &GlobSet)->Result<()> {
    if src_dir.is_dir() {
        for entry in read_dir(src_dir)? {
            let entry = entry?;
            let path = entry.path();

            if !excludes.is_match( &path) {
                if let Some(fname) = path.file_name() {
                    let tgt_path = tgt_dir.join(fname);

                    if path.is_dir() {
                        println!("-- duplicating {:?} <- {:?}/", tgt_path, path);

                        ensure_writable_dir(&tgt_path)?;
                        duplicate_dir( &path, &tgt_path, excludes);
        
                    } else {
                        if ! (tgt_path.is_file() || tgt_path.is_symlink()) { 
                            if ARGS.link_files {
                                println!("link {:?} -> {:?}", tgt_path, path);
                                let abs_path = path.canonicalize()?;
                                sym_link( &abs_path, &tgt_path)?;
                            } else {
                                println!("copy {:?} <- {:?}", tgt_path, path );
                                copy( &path, &tgt_path)?;
                            }
                        } else {
                            println!("skip {:?}", path);
                        }
                    }
                }
            }
        }
        Ok(())

    } else {
        Err( anyhow!("source is not a directory: {:?}", src_dir) )
    }
}

#[cfg(target_os = "windows")]
fn sym_link<P: AsRef<Path>, Q: AsRef<Path>> (src: P, tgt: Q)->Result<()> {
    use std::os::windows::fs;
    Ok( fs::symlink_file( src, tgt)? )
}

#[cfg(any( target_os = "linux", target_os = "openbsd", target_os = "freebsd", target_os = "aix", target_os = "macos"))]  // should probably cover more
fn sym_link<P: AsRef<Path>, Q: AsRef<Path>> (src: P, tgt: Q)->Result<()> {
    use std::os::unix::fs;
    Ok( fs::symlink( src, tgt)? )
}
