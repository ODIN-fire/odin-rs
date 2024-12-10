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

// re-export these so that macro callers don't have import them explicitly (they don't see the macro implementation)
pub extern crate bytes;
pub extern crate lazy_static;

use std::{io::Write,fs::{self,File},env,path::{Path,PathBuf},fmt::Write as FmtWrite, str, collections::HashMap, sync::Arc};
use minifier::{js,css,json};  // js, json, css
use minify_html_onepass::{Cfg, truncate};  // html, svg
use bytes::Bytes;
use crate::*; // this includes utils
use crate::errors::*;

pub const ASSETS: &'static str = "assets";

/// aggregate to specify both raw source and processing steps for embedded assets (based on manifest resource definition)
pub struct EmbeddedAssetEntry {
    pub src: Bytes, // we keep this as Bytes object so that we can unify filesystem- and embedded asset data
    pub is_encrypted: bool,
}

/// build script part of asset management
/// generate a target/❬build-type❭/build/❬build-hash❭/out/asset_data file that is included in respective config crates.
/// This function is called from build.rs which means we don't have a odin_build::BIN_CONTEXT and have to get
/// potential bin_name and bin_crate settings from ODIN_.. environment vars
pub fn create_asset_data ()->Result<()> {
    use std::fmt::Write as _;

    let embed_resources = is_env_enabled("ODIN_EMBED_RESOURCES");
    let mut out_path = Path::new( env::var("OUT_DIR")?.as_str()).to_path_buf();
    out_path.push( "asset_data");
    let mut buf = String::with_capacity(1024 * 32);
    let mut n_resources = 0;  // number of processed resources
    let bin_ctx = get_env_bin_context();
    let resource_crate = env::var("CARGO_PKG_NAME")?;

    write!( buf, "|map: &mut std::collections::HashMap<&'static str, odin_build::EmbeddedAssetEntry>| {{\n");

    if embed_resources {
        info!("---- embedding defined asset resources of crate {}", resource_crate);
        let manifest = load_manifest()?;
        if let Some(meta) = get_metadata(&manifest) {
            let resources = &meta.odin_assets;
            for (key,resource) in resources.iter() {
                if is_relevant_resource(&resource, &bin_ctx) {
                    let path = if let Some(dir) = &resource.dir { 
                        dir.join( &resource.file)
                    } else { // we do need the app_name / app_crate
                        find_resource_file( ASSETS, &bin_ctx.as_ref(), &resource_crate, &resource.file).unwrap()
                    };

                    info!("embedding {} from {:?}", resource.file, path);
                    if !path.is_file() { error!("file does not exist: {:?}", path) }

                    let data = file_contents_as_bytes(&path)?;
                    let processed_data = process_asset_resource( &resource, data)?;

                    write!( buf, "static _D{}_: &[u8] = &{:?};\n", n_resources, processed_data);
                    write!( buf, "map.insert( \"{}\", odin_build::EmbeddedAssetEntry{{ src: Bytes::from(_D{}_), is_encrypted:{} ));\n", 
                            resource.file, n_resources, resource.encrypt);
                    n_resources += 1;
                }
            }
        } else {
            info!("no ODIN metadata found in Cargo.toml");
        }
    }

    write!( buf, "}}");
    write_file( &out_path, buf.as_bytes())?;
    if n_resources > 0 { 
        if embed_resources { info!("generated {out_path:?}"); } 
    }

    Ok(())
}


/// runtime part of asset management
/// this is the main macro that needs to be expanded at the top of crates (lib.rs) that define assets.
#[macro_export]
macro_rules! define_load_asset {
    () => {
        mod assets {
            use std::{collections::HashMap,sync::Mutex,path::Path};
            use $crate::lazy_static::lazy_static;
            use $crate::bytes::Bytes;

            lazy_static! {
                // embedded assets are the ones we compiled into the (standalone) application
                static ref EMBEDDED_ASSETS: HashMap<&'static str, odin_build::EmbeddedAssetEntry> = {
                    let mut map: HashMap<&'static str, odin_build::EmbeddedAssetEntry> = HashMap::new();
                    
                    #[cfg(feature="embedded_resources")]
                    include!(concat!(env!("OUT_DIR"), "/asset_data")) (&mut map);
                    
                    map
                };

                // cached fs assets are the ones we had previously loaded from the fs (if ODIN_RELOAD_ASSETS is not set)
                // note this is already processed data
                static ref CACHED_FS_ASSETS: Mutex<HashMap<String, Option<Bytes>>> = Mutex::new(HashMap::new());
            }

            pub fn load_asset (filename: &str) -> odin_build::Result<Bytes> {
                use std::sync::Arc;

                let bin_ctx = odin_build::BIN_CONTEXT.get();
                let resource_crate = env!("CARGO_PKG_NAME");
                let reload = odin_build::is_env_enabled("ODIN_RELOAD_ASSETS");
                let mut fs_checked = false;

                // only do filesytem lookup if ODIN_EMBEDDED_ONLY env var is not enabled at runtime (set to 1|true|on)
                if !odin_build::is_env_enabled("ODIN_EMBEDDED_ONLY") {
                    if !reload { // check if we already loaded it from file
                        if let Ok(cache) = CACHED_FS_ASSETS.lock() {
                            if let Some(maybe_data) = cache.get(filename) { // we have checked the fs before
                                if let Some(data) = maybe_data {
                                    return Ok( data.clone() );
                                } else { // we previously didn't find it in the fs, don't check again
                                    fs_checked = true;
                                }
                            }
                        }
                    }

                    if !fs_checked {
                        if let Some(path) = odin_build::find_asset_file( &bin_ctx, resource_crate, filename) {
                            let data = odin_build::file_contents_as_bytes(&path)?;
                            let proc_data = odin_build::process_asset( filename, data)?;
                            let bytes = Bytes::from(proc_data);

                            if !reload {
                                if let Ok(mut cache) = CACHED_FS_ASSETS.lock() {
                                    cache.insert( filename.to_string(), Some(bytes.clone()));
                                }
                            }

                            return Ok( bytes )

                        } else { // we didn't find it in the file system
                            if !reload {
                                if let Ok(mut cache) = CACHED_FS_ASSETS.lock() {
                                    cache.insert( filename.to_string(), None);
                                }
                            }
                        }
                    }
                }

                if let Some(entry) = EMBEDDED_ASSETS.get( filename) {
                    // ... this is where EmbeddedAssetEntry attributes would get processed
                    Ok( entry.src.clone() ) // it is stored in processed form (e.g. compressed)
                } else {
                    Err( odin_build::OdinBuildError::ResourceNotFoundError(filename.to_string()) )
                }
            }

            // TODO - this should also check/honor ODIN_RELOAD_ASSETS
            pub fn load_asset_path (path: impl AsRef<Path>) -> odin_build::Result<Bytes> {
                let path = path.as_ref();
                if let Some(filename) = path.file_name() {
                    if let Some(filename) = filename.to_str() {
                        let data = odin_build::file_contents_as_bytes(path)?;
                        let proc_data = odin_build::process_asset( filename, data)?;
                        return Ok( Bytes::from(proc_data) )
                    }
                }
                Err( odin_build::OdinBuildError::ResourceNotFoundError(format!("{:?}",path)) )
            }
        }
        pub use assets::*;
    }
}

pub fn find_asset_file (ctx: &Option<&BinContext>, resource_crate: &str, filename: &str) -> Option<PathBuf> {
    find_resource_file( ASSETS, ctx, resource_crate, filename)
}


// we don't capture the compression status separately but compress html/svg, css, js and json with brotli.
// should a request not allow for compression we decompress on-the-fly.
// content type is deduced from the filename of the asset.

fn process_asset_resource (resource: &Resource, data: Vec<u8>) -> Result<Vec<u8>> {
    process_asset( resource.file.as_str(), data)
}

pub fn process_asset (filename: &str, data: Vec<u8>) -> Result<Vec<u8>> {
    if let Some(ext) = extension(filename) {
        match ext {
            "html" => process_html(data),
            "css"  => process_css(data),
            "svg"  => process_svg(data),
            "js"   => {
                if filename.ends_with(".min.js") {
                    process_compressed(data) // don't minify
                } else {
                    process_js(data)
                }
            }
            "json" => {
                if filename.ends_with(".uncompressed.json") {
                    process_json_x(data)
                } else {
                    process_json(data)
                }
            }
            "xml"  => process_xml(data),
            "csv"  => process_csv(data),
            "txt"  => process_txt(data),

            "jpeg" | "png" | "webp" | "tif" | "mp4" | "mpeg" | "webm" | "weba" => Ok(data),
            "gz"  => Ok(data),

            _ => Err( OdinBuildError::ResourceTypeError( filename.into() ) )
        }
    } else { Err( OdinBuildError::ResourceTypeError(filename.into()) ) }
}

fn process_html (mut data: Vec<u8>)->Result<Vec<u8>> {
    let cfg = &Cfg { minify_js: true, minify_css: true };
    truncate( &mut data, cfg).map_err(|e| OdinBuildError::MinifyError(e.to_string()))?;
    compress_vec( &data)
}

fn process_css (data: Vec<u8>)->Result<Vec<u8>> {
    let content = str::from_utf8(&data)?;
    let mini = css::minify(content).map_err(|e| OdinBuildError::MinifyError(e.to_string()))?.to_string();
    compress_vec( &mini.into_bytes())
}

fn process_js (data: Vec<u8>)->Result<Vec<u8>> {
    let content = str::from_utf8(&data)?;
    let mini = js::minify(content).to_string();
    compress_vec( &mini.into_bytes())

    //compress_vec(&data)  // ONLY outside repo
}

fn process_svg (data: Vec<u8>)->Result<Vec<u8>> {
    //process_html( data) // gets broken by minifier
    compress_vec( &data)
}

fn process_json (data: Vec<u8>)->Result<Vec<u8>> {
    let content = str::from_utf8(&data)?;
    let mini = json::minify(content).to_string();
    compress_vec( &mini.into_bytes())
}

// don't compress
fn process_json_x (data: Vec<u8>)->Result<Vec<u8>> {
    let content = str::from_utf8(&data)?;
    let mini = json::minify(content).to_string();
    Ok(mini.into_bytes())
}

fn process_xml (data: Vec<u8>) -> Result<Vec<u8>> {
    // TODO - find a minifier
    compress_vec( &data)
}

fn process_csv (data: Vec<u8>) -> Result<Vec<u8>> {
    compress_vec( &data)
}

fn process_txt (data: Vec<u8>) -> Result<Vec<u8>> {
    compress_vec( &data)
}

fn process_compressed (data: Vec<u8>) -> Result<Vec<u8>> {
    compress_vec( &data)
}

pub struct ContentSpec {
    pub mime_type: &'static str,
    pub encoding: Option<&'static str>,
}

pub fn get_content_spec (pathname: &str)->ContentSpec {
    let default_enc = default_encoding();

    if let Some(ext) = extension(pathname) {
        match ext {
            // our compressed asset data types
            "js"    => return ContentSpec { mime_type: "text/javascript",  encoding: default_enc },
            "js-raw" => return ContentSpec { mime_type: "text/javascript",  encoding: default_enc },
            "css"   => return ContentSpec { mime_type: "text/css",         encoding: default_enc },
            "html"  => return ContentSpec { mime_type: "text/html",        encoding: default_enc },
            "json"  => return ContentSpec { mime_type: "application/json", encoding: default_enc },
            "svg"   => return ContentSpec { mime_type: "image/svg+xml",    encoding: default_enc },
            "xml"   => return ContentSpec { mime_type: "application/xml",  encoding: default_enc },
            "csv"   => return ContentSpec { mime_type: "text/csv",         encoding: default_enc },
            "txt"   => return ContentSpec { mime_type: "text/plain",       encoding: default_enc },

            // uncompressed asset data types (those have content specific compression)
            "jpeg"  => return ContentSpec { mime_type: "image/jpeg",       encoding: None },
            "png"   => return ContentSpec { mime_type: "image/png",        encoding: None },
            "webp"  => return ContentSpec { mime_type: "image/webp",       encoding: None },
            "tif"   => return ContentSpec { mime_type: "image/tif",        encoding: None },
            "mp4"   => return ContentSpec { mime_type: "video/mp4",        encoding: None },
            "mpeg"  => return ContentSpec { mime_type: "video/mpeg",       encoding: None },
            "webm"  => return ContentSpec { mime_type: "video/webm",       encoding: None },
            "weba"  => return ContentSpec { mime_type: "audio/weba",       encoding: None },

            // ...and more to follow
            &_      => {}
        }
    }

    ContentSpec { mime_type: "application/octet-stream", encoding: None }
}