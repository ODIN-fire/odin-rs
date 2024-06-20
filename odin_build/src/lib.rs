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

//! `odin_build` is a library crate for common build.rs functions used within ODIN.
//! This is factored out here to avoid redundant code in build scripts, namely for
//! generating embedded `config_data` and `asset_data` sources

use std::{io::{Read,Write},fs::{self,File,DirEntry},env,path::{Path,PathBuf},fmt::Write as FmtWrite};
use brotli::CompressorWriter;
use anyhow::{Result,anyhow};
use minifier::{js,css,json};  // js, json, css
use minify_html_onepass::{Cfg, truncate};  // html, svg


/* #region common macros and functions **********************************************************/

// this is a hack to avoid the warning output from Cargo. Hopefully Cargo will some day support build script messages directly

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

fn visit_dirs(dir: impl AsRef<Path>, f: &mut dyn FnMut(&DirEntry)) -> Result<()> {
    let dir = dir.as_ref();
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                visit_dirs(&path, f)?;
            } else {
                f(&entry);
            }
        }
    }
    Ok(())
}

fn read_to_string (path: &Path)->Result<String> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut content = String::with_capacity(len as usize);
    file.read_to_string(&mut content)?;
    Ok(content)
}

fn brotli_compress_vec (v_in: &Vec<u8>)->Result<Vec<u8>> {
    let v_out: Vec<u8> = Vec::with_capacity( v_in.len() / 4);
    let v_in = v_in.as_slice();
    let mut writer = CompressorWriter::new( v_out, v_in.len(), 11,22);
    writer.write_all(v_in)?;
    writer.flush()?;
    let v_out = writer.into_inner();
    Ok(v_out)
}

/* #endregion common macros and functions */


/* #region asset_data ***************************************************************************/

/// build script function to generate 'asset_data' sources for known content under ../assets of the current crate
/// 
/// Those fragments can be used to include/compile static binaries of respective files, e.g. to be served via ServeMem
/// The format is as follows:
/// ```
/// pub static FOO_JS: &'static[u8] = &[ ... ];
/// ...
/// ```
/// Note that services still have to explicitly add respective assets they want to inline (within their `add_components(..)` functions).
/// We use separate static items so that link time optimization can remove the ones that shouldn't be inlined
pub fn generate_asset_data() -> Result<()> {
    let crate_name = env::var("CARGO_CRATE_NAME").unwrap();
    let out_dir = env::var("OUT_DIR").unwrap(); // this is always be defined by cargo
    let in_dir = env::var("IN_DIR").unwrap_or("assets".into());
    let out_file: PathBuf = [out_dir.as_str(), "asset_data"].iter().collect();
    let mut file = File::create(&out_file)?;
    let mut asset_data = String::with_capacity(4096);

    write!( &mut asset_data, "/// embedded asset data for crate {}", crate_name)?;

    if Path::new(in_dir.as_str()).is_dir() {
        visit_dirs( &in_dir, &mut |e| {
            let path = e.path();
            let filename = path.file_name().unwrap().to_string_lossy();
            info!("inlining asset {path:?}");

            if let Ok(data) = process_asset(&path) {
                let name = filename.replace(".", "_").replace("-", "_").to_uppercase();
                write!( &mut asset_data, "pub static {}: &'static[u8] = &{:?};\n", name, data).unwrap();
            } else { 
                warn!("failed to read/compress asset {filename}") 
            }
        })?;
    }
    file.write_all( asset_data.as_bytes())?;
    
    info!("created {:?}", out_file);
    Ok(())
}



// we don't capture the compression status separately but compress html/svg, css, js and json with brotli.
// should a request not allow for compression we decompress on-the-fly.
// content type is deduced from the filename of the asset.
// jpeg and png are not compressed

fn process_asset (path: &Path)->Result<Vec<u8>> {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        match ext {
            "html" => process_html(path),
            "css" => process_css(path),
            "svg" => process_svg(path),
            "js" => process_js(path),
            "json" => process_json(path),
            "jpeg" => Ok( fs::read(path)? ),
            "png" => Ok( fs::read(path)? ),
            _ => Err( anyhow!( "unsupported asset type") )
        }
    } else { Err( anyhow!( "no asset type") ) }
}

fn process_js (path: &Path)->Result<Vec<u8>> {
    let content = read_to_string(path)?;
    let mini = js::minify(&content).to_string();
    brotli_compress_vec( &mini.into_bytes())
}

fn process_html (path: &Path)->Result<Vec<u8>> {
    let mut data = fs::read( path)?;
    let cfg = &Cfg { minify_js: true, minify_css: true };
    truncate( &mut data, cfg)?;
    brotli_compress_vec( &data)
}

fn process_svg (path: &Path)->Result<Vec<u8>> {
    process_html( path)
}

fn process_css (path: &Path)->Result<Vec<u8>> {
    let content = read_to_string(path)?;
    let mini = css::minify(&content).map_err(|e| anyhow!("{e}"))?.to_string();
    brotli_compress_vec( &mini.into_bytes())
}

fn process_json (path: &Path)->Result<Vec<u8>> {
    let content = read_to_string(path)?;
    let mini = json::minify(&content).to_string();
    brotli_compress_vec( &mini.into_bytes())
}

/* #endregion asset_data */