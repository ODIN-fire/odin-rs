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

#[doc = include_str!("../doc/odin_build.md")]

use std::{fs, path::{Path,PathBuf}, sync::{Arc,OnceLock}, env, collections::HashMap};

pub mod prelude;

mod assets;
pub use assets::*;

mod configs;
pub use configs::*;

mod manifest;
use manifest::*;

//mod encrypt;
//pub use encrypt::*;

mod utils;
pub use utils::*;

mod errors;
pub use errors::*;

pub type OdinBuildResult<T> = errors::Result<T>;
pub type LoadAssetFp = fn(&str)->OdinBuildResult<bytes::Bytes>;

/// this has to be called from build.rs to make sure we re-run the build script if any of the env vars changes
pub fn init_build() {
    println!("cargo::rerun-if-env-changed=ODIN_EMBED_RESOURCES");
    println!("cargo::rerun-if-env-changed=ODIN_RESOURCES");
    println!("cargo::rerun-if-env-changed=ODIN_BIN_NAME");
    println!("cargo::rerun-if-env-changed=ODIN_BIN_CRATE");
}

/* #region logging and debugging *********************************************************/

#[macro_export]
macro_rules! info {
    ($($tokens: tt)*) => {
        println!("cargo:warning=\r\x1b[32;1m  \x1b[37m info: {}\x1b[0m", format!($($tokens)*))
    }
}

#[macro_export]
macro_rules! warn {
    ($($tokens: tt)*) => {
        println!("cargo:warning=\r\x1b[32;1m  \x1b[93m warn: {}\x1b[0m", format!($($tokens)*))
    }
}

#[macro_export]
macro_rules! error {
    ($($tokens: tt)*) => {
        println!("cargo:warning=\r\x1b[32;1m  \x1b[91m error: {}\x1b[0m", format!($($tokens)*))
    }
}

/* #endregion logging and debugging */

/* #region bin globals *******************************************************************/

#[derive(Debug)]
pub struct BinContext {
    pub bin_name: String,
    pub bin_crate: String,
    pub bin_suffix: Option<String>, // optionally set via ODIN_BIN_SUFFIX at runtime (useful if we run simultaneous instances of this bin)
    pub proc_id: Option<u32>,

    pub build: String, // describing how binary was built (showing build-time env settings)
}

pub static BIN_CONTEXT: OnceLock<BinContext> = OnceLock::new();

/// this has to be called (once) from the bin source
#[macro_export]
macro_rules! set_bin_context {
    () => {
        {
            let bin_crate = env!("CARGO_PKG_NAME").to_string(); // those are only set at compile time hence this needs a macro
            let bin_name = env!("CARGO_BIN_NAME").to_string();
            let bin_suffix = std::env::var("ODIN_BIN_SUFFIX").ok(); // NOTE this is a runtime env var
            let proc_id = Some(std::process::id());
            let build = odin_build::build_mode!();

            odin_build::BIN_CONTEXT.set( odin_build::BinContext{ bin_name, bin_crate, bin_suffix, proc_id, build } ).unwrap();
        }
    }
}

#[macro_export]
macro_rules! build_mode {
    () => {
        {
            use std::fmt::Write;
            let mut build = String::new();
            if let Some(v) = option_env!("ODIN_EMBED_RESOURCES") { 
                write!( build, "embed={}", v).unwrap(); 
            }
            //... and more to follow
            build
        }
    }
}

pub fn get_bin_context()->Option<&'static BinContext> {
    BIN_CONTEXT.get()
}

/// get a an optional BinContext from environment variables. Called from build.rs
pub fn get_env_bin_context()->Option<BinContext> {
    let bin_name = env::var("ODIN_BIN_NAME");
    let bin_crate = env::var("ODIN_BIN_CRATE");
    let bin_suffix = env::var("ODIN_BIN_SUFFIX").ok();

    if bin_name.is_ok() && bin_crate.is_ok() {
        let bin_name = bin_name.unwrap();
        let bin_crate = bin_crate.unwrap();
        let proc_id = None;
        let build = build_mode!();
        Some( BinContext { bin_name, bin_crate, bin_suffix, proc_id, build } )  // this is build-time - we don't have a proc_id yet
    } else { 
        None
    }
}

pub fn is_relevant_resource (rsc: &Resource, maybe_ctx: &Option<BinContext>)->bool {
    if rsc.bins.is_empty() { 
        true 
    } else {
        if let Some(ctx) = maybe_ctx {
            rsc.bins.contains(&ctx.bin_name)
        } else {
            true
        }
    }
}

/// this is mostly for examples within crates that do not have their own define_load_config
pub fn load_config_path<C,P> (path: P) -> Result<C> where C: for <'a> serde::Deserialize<'a>, P: AsRef<Path> {
    let data = file_contents_as_bytes(path.as_ref())?;
    Ok( ron::de::from_bytes( data.as_slice())? )
}


// the global ODIN dirs of the application, which are invariant after init
// we don't have a global CONFIG_DIR or ASSET_DIR since respective resources can reside in a number of locations
// (including in-memory).
pub static ROOT_DIR: OnceLock<PathBuf> = OnceLock::new();
pub static CACHE_DIR: OnceLock<PathBuf> = OnceLock::new();
pub static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn root_dir()->&'static PathBuf {
    ROOT_DIR.get_or_init(|| get_or_create_root_dir().expect("failed to locate ODIN root"))
}

pub fn data_dir()->&'static PathBuf {
    DATA_DIR.get_or_init(|| ensure_existing_path( root_dir().join( Path::new("data"))))
}

pub fn cache_dir()->&'static PathBuf {
    CACHE_DIR.get_or_init(|| ensure_existing_path( root_dir().join( Path::new("cache"))))
}

// those need to be compiled in the target crate hence we need macros

#[macro_export]
macro_rules! pkg_cache_dir {
    () => {
        odin_build::ensure_dir( odin_build::cache_dir().join( env!("CARGO_PKG_NAME")))
    }
}

#[macro_export]
macro_rules! pkg_data_dir {
    () => {
        odin_build::ensure_dir( odin_build::cache_dir().join( env!("CARGO_PKG_NAME")))
    }
}

/// Note - this panics if the directory does not exist and can't be created
pub fn ensure_dir (dir: PathBuf)->PathBuf {
    if !&dir.is_dir() { 
        std::fs::create_dir_all(&dir).unwrap(); 
    }
    dir
}



/* #endregion bin globals */

/* #region resource lookup ***************************************************************/

/// locate a config file and return its PathBuf 
/// note this is called both from build.rs from load_config (at runtime) so we have to explicitly pass in BinContext
fn find_resource_file (resource_dir: &str, ctx: &Option<&BinContext>, resource_crate: &str, filename: &str) -> Option<PathBuf> {
    // check an explicit ODIN_HOME first
    if let Ok(odin_home) = env::var("ODIN_HOME") {
        let mut path = Path::new( odin_home.as_str()).to_path_buf();
        if find_external_resource( &mut path, resource_dir, ctx, resource_crate, filename) { return Some(path) }
    }

    // try the parent of the workspace dir next - this is the first dir outside the source repo
    if let Some(mut path) = get_workspace_parent() {
        if find_external_resource( &mut path, resource_dir, ctx, resource_crate, filename) { return Some(path) }
    }

    // as a last resort try an implicit ~/.odin/CONFIG_DIR
    if let Ok(usr_home) = env::var("HOME") {
        let mut path = Path::new(usr_home.as_str()).to_path_buf();
        path.push(".odin");
        if find_external_resource( &mut path, resource_dir, ctx, resource_crate, filename) { return Some(path) }
    }

    // try to find the config within the repo
    if let Some(mut path) = get_workspace_dir() {
        if find_internal_resource( &mut path, resource_dir, ctx, resource_crate, filename) { return Some(path) }
    }

    None
}

fn find_external_resource (path: &mut PathBuf, resource_dir: &str, bin_ctx: &Option<&BinContext>, resource_crate: &str, filename: &str)->bool {

    // check bin specific override first
    if let Some(ctx) = bin_ctx {
        let bin_crate = ctx.bin_crate.as_str();
        let bin_name = ctx.bin_name.as_str();
        if path_cond!( is_file, path, resource_dir, bin_crate, bin_name, resource_crate, filename) { return true }
    }

    // now check resource crate global
    if path_cond!( is_file, path, resource_dir, resource_crate, filename) { return true }
    
    false
}

fn find_internal_resource (path: &mut PathBuf, resource_dir: &str, bin_ctx: &Option<&BinContext>, resource_crate: &str, filename: &str)->bool {
    if let Some(ctx) = bin_ctx {
        let bin_crate = ctx.bin_crate.as_str();
        let bin_name = ctx.bin_name.as_str();
        if path_cond!( is_file, path, bin_crate, resource_dir, bin_name, resource_crate, filename) { return true }
    }

    if path_cond!( is_file, path, resource_crate, resource_dir, filename) { return true }
    
    false
}

/* #endregion resource lookup */
