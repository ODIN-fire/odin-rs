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

//! Cargo.toml ODIN metadata support
//! this supports two kinds of inlined resources:
//! 
//! - assets (served files)
//! - configs (serialized config objects)
//! 
//! all inlined data gets generated from respective Cargo.toml metadata entries. It is included in respective
//! `assets.rs` and `configs.rs` modules of the target crate.
//! 
//! Inlined data is always compressed and might be optionally encrypted (based on a global ODIN_ENCRYPT env var).
//! This is to ensure all crate of a build use the same encryption method

#![allow(unused)]

use serde::{Deserialize,Deserializer};
use std::{collections::HashMap, path::{Path,PathBuf}, env};
use toml::{Value,Table};
use cargo_toml::{Manifest};

use crate::{info,warn,error,errors::{OdinBuildError, Result}, utils};
use crate::utils::path_to_string;

pub type OdinManifest = Manifest<OdinMetaData>;

/// ODIN metadata represenation of a resource file (config or asset)
#[derive(Deserialize,Debug)]
pub struct Resource {
    pub file: String,

    /// optional directory of resource file. if not given it will be looked up with `odin_build::locate_resource_file()`
    #[serde(deserialize_with="deserialize_expand_path",default="default_dir")]
    pub dir: Option<PathBuf>,

    #[serde(default="default_encryption")]
    pub encrypt: bool,

    #[serde(default="default_compression")]
    pub compress: bool,

    /// optional list of bin targets for which this resource should be embedded
    #[serde(default="default_bins")]
    pub bins: Vec<String>,
}

/// the structure that captures ODIN specific Cargo manifest metadata
#[derive(Deserialize,Debug)]
pub struct OdinMetaData {
    #[serde(default="default_resource")]
    pub odin_configs: HashMap<String, Resource>,

    #[serde(default="default_resource")]
    pub odin_assets: HashMap<String,Resource>
}

fn default_encryption()->bool { false }
fn default_compression()->bool { true }
fn default_dir()->Option<PathBuf> { None }
fn default_bins()->Vec<String> { Vec::with_capacity(0) }

fn default_resource()->HashMap<String,Resource> { HashMap::new() }

pub fn deserialize_expand_path <'a,D>(deserializer: D) -> std::result::Result<Option<PathBuf>,D::Error> where D: Deserializer<'a> {
    String::deserialize(deserializer).and_then( |string| {
        let mut res: PathBuf = PathBuf::new();
        let path = Path::new(string.as_str());

        for e in path.iter() {
            let s = e.to_str().unwrap();
            if s.starts_with("$") {
                if let Ok(se) = env::var(&s[1..]) {
                    res.push(se)
                } else { res.push(s) }
            } else { res.push(s) }
        }

        Ok(Some(res))
    })
}

/// this is called at build time - it's Ok to rely on CARGO_.. envs)
pub fn load_manifest() -> Result<OdinManifest> {
    let mut path = Path::new( env::var("CARGO_MANIFEST_DIR")?.as_str()).to_path_buf();
    path.push("Cargo.toml");

    let cargo_toml = utils::file_contents_as_bytes(&path)?;
    Ok( Manifest::<OdinMetaData>::from_slice_with_metadata( cargo_toml.as_slice())? )
}

pub fn get_metadata<'a>  (manifest: &'a OdinManifest) -> Option<&'a OdinMetaData> {
    manifest.package().metadata.as_ref()
}
