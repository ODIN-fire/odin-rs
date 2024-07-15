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

#![allow(unused)]

use std::{env,path::{Path,PathBuf}};
use crate::*; // this includes utils
use crate::errors::*;

pub const CONFIGS: &'static str = "configs";

/// aggregate to specify both raw source and processing steps for embedded configs (based on manifest resource definition)
pub struct EmbeddedConfigEntry {
    pub src: &'static [u8],
    pub is_encrypted: bool,
}

pub fn find_config_file (ctx: &Option<&BinContext>, resource_crate: &str, filename: &str) -> Option<PathBuf> {
    find_resource_file( CONFIGS, ctx, resource_crate, filename)
}

/// build script part of config management
/// generate a target/❬build-type❭/build/❬build-hash❭/out/config_data file that is included in respective config crates.
/// This function is called from build.rs which means we don't have a odin_build::BIN_CONTEXT and have to get
/// potential bin_name and bin_crate settings from ODIN_.. environment vars
pub fn create_config_data ()->Result<()> {
    use std::fmt::Write as _;

    let embed_resources = is_env_enabled("ODIN_EMBED_RESOURCES");
    let mut out_path = Path::new( env::var("OUT_DIR")?.as_str()).to_path_buf();
    out_path.push( "config_data");
    let mut buf = String::with_capacity(4096);
    let mut n_resources = 0;  // number of processed resources
    let bin_ctx = get_env_bin_context();
    let resource_crate = env::var("CARGO_PKG_NAME")?;

    write!( buf, "|map: &mut std::collections::HashMap<&'static str, odin_build::EmbeddedConfigEntry>| {{\n");

    if embed_resources {
        info!("---- embedding defined config resources of crate {}", resource_crate);
        let manifest = load_manifest()?;
        if let Some(meta) = get_metadata(&manifest) {
            let resources = &meta.odin_configs;
            for (key,resource) in resources.iter() {
                if is_relevant_resource(&resource, &bin_ctx) {
                    let path = if let Some(dir) = &resource.dir { 
                        dir.join( &resource.file)
                    } else { // we do need the app_name / app_crate
                        match find_resource_file( CONFIGS, &bin_ctx.as_ref(), &resource_crate, &resource.file) {
                            Some(path) => path,
                            None => panic!("failed to locate embedded resource \"{}\" for crate {}", resource.file, resource_crate)
                        }
                    };

                    info!("embedding {} from {:?}", resource.file, path);
                    if !path.is_file() { error!("file does not exist: {:?}", path) }

                    let data = utils::file_contents_as_bytes(&path)?;
                    let processed_data = process_config_resource( &resource, data)?;

                    write!( buf, "static _D{}_: &[u8] = &{:?};\n", n_resources, processed_data);
                    write!( buf, "map.insert( \"{}\", odin_build::EmbeddedConfigEntry{{ src:_D{}_,is_encrypted:{} }} );\n", 
                            resource.file, n_resources, resource.encrypt);
                    //write!( buf, "map.insert( \"{}\", vec!{:?} );\n", resource.file, processed_data);
                    n_resources += 1;
                }
            }
        } else {
            info!("no ODIN metadata found in Cargo.toml");
        }
    }

    write!( buf, "}}");
    utils::write_file( &out_path, buf.as_bytes())?;
    if n_resources > 0 { 
        if embed_resources { info!("generated {out_path:?}"); } 
    }

    Ok(())
}

fn process_config_resource (r: &Resource, v: Vec<u8>)->Result<Vec<u8>> { 
    utils::compress_vec( v.as_slice()) 
}

/// runtime (crate) part of config management
/// this is the main macro that needs to be expanded at the top of crates (lib.rs) that define configs.
/// Config users call the defined `load_config(..)` function to instantiate config structs
#[macro_export]
macro_rules! define_load_config {
    // odin_build is already imported in the target or otherwise this macro wouldn't be visible

    () => {
        mod configs {
            use lazy_static::lazy_static;
            use std::{collections::HashMap,path::Path};
            use serde::Deserialize;
            use ron;

            lazy_static! { // this is module-private
                static ref EMBEDDED_CONFIGS: HashMap<&'static str, odin_build::EmbeddedConfigEntry> = {
                    let mut map: HashMap<&'static str, odin_build::EmbeddedConfigEntry> = HashMap::new();
                    
                    #[cfg(feature="embedded_resources")]
                    include!(concat!(env!("OUT_DIR"), "/config_data")) (&mut map);
                    
                    map
                };
            }

            /// load config using odin_build - based lookup mechanism
            pub fn load_config<C> (filename: &str) -> odin_build::Result<C> where C: for <'a> serde::Deserialize<'a> {
                let bin_ctx = odin_build::BIN_CONTEXT.get();
                let resource_crate = env!("CARGO_PKG_NAME");

                // only do filesytem lookup if ODIN_EMBEDDED_ONLY env var is not enabled at runtime (set to 1|true|on)
                if !odin_build::is_env_enabled("ODIN_EMBEDDED_ONLY") {
                    if let Some(path) = odin_build::find_config_file( &bin_ctx, resource_crate, filename) {
                        let data = odin_build::file_contents_as_bytes(&path)?;
                        return Ok( ron::de::from_bytes( data.as_slice())? )
                    }
                }

                if let Some(ce) = EMBEDDED_CONFIGS.get( filename) {
                    let data = odin_build::decompress_vec( ce.src)?;
                    // ... this is where additional EmbeddedConfigEntry attribute processing (decryption etc) would take place
                    return Ok( ron::de::from_bytes( data.as_slice())? )
                }

                Err( odin_build::OdinBuildError::ResourceNotFoundError(filename.to_string()) )
            }
        }
        pub use configs::*; // make load_config() visible at the crate level
    }
}

