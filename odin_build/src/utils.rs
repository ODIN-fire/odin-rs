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

use serde::{Deserialize,Deserializer};
use std::{io::{Read,Write},path::{Path,PathBuf},fs::{self,File,DirEntry},env};
use crate::errors::Result;
use brotli::{CompressorWriter,BrotliDecompress};
use flate2::{Compression,write::GzEncoder,read::GzDecoder};


pub fn path_to_string (path: impl AsRef<Path>)->String {
    path.as_ref().to_str().unwrap().to_string()
}

pub fn file_contents_as_bytes (path: impl AsRef<Path>) -> Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut contents: Vec<u8> = Vec::with_capacity(len as usize);
    file.read_to_end(&mut contents)?;
    Ok(contents)
}

pub fn write_file (path: impl AsRef<Path>, contents: &[u8]) -> Result<()> {
    let mut file = File::create( path)?;
    Ok( file.write_all(contents)? )
}

/************* choose either br or gz based on feature  ***********************************/
/// we use Brotli compression for all respective resources
pub fn br_compress_vec (v_in: &[u8]) -> Result<Vec<u8>> {
    let v_out: Vec<u8> = Vec::with_capacity( v_in.len() / 4);
    let mut writer = CompressorWriter::new( v_out, v_in.len(), 11,22);
    writer.write_all(v_in)?;
    writer.flush()?;
    let v_out = writer.into_inner();
    Ok( v_out )
}

pub fn br_decompress_vec (v_in: &[u8]) -> Result<Vec<u8>> {
    let mut v_out = Vec::with_capacity( v_in.len() * 5);
    let mut input = v_in;
    BrotliDecompress( &mut input, &mut v_out)?;
    Ok( v_out )
}

pub fn gz_compress_vec (v_in: &[u8]) -> Result<Vec<u8>> {
    let v_out: Vec<u8> = Vec::with_capacity( v_in.len() / 4);
    let mut encoder = GzEncoder::new(v_out, Compression::default());
    encoder.write_all( &v_in)?;
    Ok( encoder.finish()? )
}

pub fn gz_decompress_vec (v_in: &[u8]) -> Result<Vec<u8>> {
    let mut v_out = Vec::with_capacity( v_in.len() * 5);
    let mut decoder = GzDecoder::new(v_in);
    decoder.read_to_end( &mut v_out)?;
    // v_out.shrink_to_fit();
    Ok( v_out )
}

#[inline]
pub fn default_encoding()->Option<&'static str> { Some("gzip") }

#[inline]
pub fn compress_vec (v_in: &[u8]) -> Result<Vec<u8>> { gz_compress_vec(v_in) }

#[inline]
pub fn decompress_vec (v_in: &[u8]) -> Result<Vec<u8>> { gz_decompress_vec(v_in) }

/*********** end feature *******************************************************************/

pub fn visit_dirs(dir: impl AsRef<Path>, f: &mut dyn FnMut(&DirEntry)) -> Result<()> {
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

pub fn read_to_string (path: &Path)->Result<String> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut content = String::with_capacity(len as usize);
    file.read_to_string(&mut content)?;
    Ok(content)
}

pub fn deserialize_expand_path <'a,D>(deserializer: D) -> std::result::Result<PathBuf,D::Error> where D: Deserializer<'a> {
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

        Ok(res)
    })
}

/// modify path and check if path condition holds. If not revert the path to its previous state
#[macro_export]
macro_rules! path_cond {
    ( $pred:ident, $path_expr:expr, $($e:expr),* ) => {
        {
            let path: &mut PathBuf = $path_expr;
            let n0 = path.components().count();
            $( path.push($e); )*
            if path.$pred() { 
                true
            } else {
                // restore path
                let mut n = path.components().count();
                while n > n0 { path.pop(); n -= 1; }
                false
            }
        }
    }
}

#[macro_export]
macro_rules! has_any_path_cond {
    ($pred:ident, $path_expr:expr, $($e:expr),*) => {
        {
            let mut path: &mut PathBuf = $path_expr;
            let mut holds = |e| { path.push(e); let res=path.$pred(); path.pop(); res };
            $( holds($e) || )* false
        }
    }
}

/// this is the highest parent from the current dir that still has a Cargo.toml
pub fn get_workspace_dir()->Option<PathBuf> {
    if let Ok(mut path) = env::current_dir() {
        while path_cond!( is_file, &mut path, "..", "Cargo.toml") {
            path.pop(); // pops Cargo.toml
            path.pop(); // pops ".."
            if !path.pop() { return None } // no parent
        }
        return Some(path)
    }
    None
}

pub fn get_workspace_parent()->Option<PathBuf> {
    get_workspace_dir().map( |mut p| { p.pop(); p})
}

pub fn get_env_odin_root()->Option<PathBuf> {
    if let Ok(odin_root) = env::var("ODIN_ROOT") {
        Some( Path::new(odin_root.as_str()).to_path_buf() )
    } else { None }
}

pub fn default_odin_root()->PathBuf {
    let mut path = Path::new( env::var("HOME").unwrap().as_str()).to_path_buf();
        path.push( ".odin");
        path
}

/// get the ODIN root dir to use. If this returns Ok the path is guaranteed to exist.
/// Lookup is in the following order:
/// 
/// 1. use $ODIN_ROOT if set
/// 2. workspace parent if it has any of the odin dirs {cache,data,configs,assets}
/// 3. $HOME/.odin
pub fn get_or_create_root_dir()->Result<PathBuf> {
    let mut path = if let Some(path) = get_env_odin_root() {
        path

    } else {
        let computed_path = if let Some(mut path) = get_workspace_parent() {
            if has_any_path_cond!( is_dir, &mut path, "cache", "data", "configs", "assets") {
                path
            } else {
                default_odin_root()
            }
        } else {
            default_odin_root()
        };
        // automatically set ODIN_ROOT to the computed path for the current process and its children
        // NOTE - this is not multi-threaded. Caller has to make sure this assumption holds
        unsafe {
            env::set_var("ODIN_ROOT", &computed_path);
        }

        computed_path
    };

    Ok( ensure_existing_path(path) )
}

pub fn ensure_existing_path<P> (path: P)->P where P: AsRef<Path> {
    let p = path.as_ref();
    if !p.is_dir() { 
        fs::create_dir_all(p).expect(&format!("failed to create {:?}", p)); 
    }
    path
}

#[macro_export]
macro_rules! crate_name {
    () => { env!("CARGO_PKG_NAME") }
}

pub fn is_env_enabled (key: &'static str)->bool {
    match env::var(key) {
        Ok(v) =>  v == "1" || v == "true" || v == "on",
        _ => false
    }
}

pub fn extension (path: &str)->Option<&str> {
    if let Some(idx) = path.rfind('.') {
        if idx < path.len()-1 { 
            return Some( path[idx+1..].as_ref() )
        }
    }
    None
}