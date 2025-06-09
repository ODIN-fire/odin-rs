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

use std::alloc::System;
use std::cmp::max;
use std::fs::{self,DirEntry,FileTimes,File,OpenOptions};
use std::io::{self,Read, Write, Error as IOError,ErrorKind};
use std::time::{SystemTime,Duration};
use std::env;
use io::ErrorKind::*;
use regex::Regex;
use std::path::{Path,PathBuf};
use odin_build;

use crate::if_let;
use crate::macros::io_error;

type Result<T> = std::result::Result<T,std::io::Error>;

// TODO - public functions should all use AsRef<Path>

pub fn filename<'a,T: AsRef<Path>> (path: &'a T)->Option<&'a str> {
    path.as_ref().file_name().and_then(|ostr| ostr.to_str())
}

pub fn extension<'a,T: AsRef<Path>> (path: &'a T)->Option<&'a str> {
    path.as_ref().extension().and_then(|ostr| ostr.to_str())
}

pub fn filestem <'a,T: AsRef<Path>> (path: &'a T)->Option<&'a str> {
    path.as_ref().file_stem().and_then(|ostr| ostr.to_str())
}

pub fn filename_of_path (path: impl AsRef<Path>)->Result<String> {
    let path = path.as_ref();

    Ok( path.file_name()
        .ok_or(IOError::new(ErrorKind::InvalidFilename, format!(" not a valid filename {path:?}")) )?
        .to_str().ok_or(IOError::new(ErrorKind::InvalidFilename, format!("invalid char in filename {path:?}")) )?
        .to_string())
}

pub fn ensure_dir (path: impl AsRef<Path>)->io::Result<()> {
    let path = path.as_ref();
    if !path.is_dir() {
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}

/// check if dir pathname exists and is writable, try to create dir otherwise
pub fn ensure_writable_dir (path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref();
    if path.is_dir() {
        let md = fs::metadata(&path)?;
        if md.permissions().readonly() {
            Err(io_error!(PermissionDenied, "output_dir {:?} not writable", &path))
        } else {
            Ok(())
        }

    } else {
        fs::create_dir_all(path)
    }
}

pub fn filepath (dir: &str, filename: &str) -> Result<PathBuf> {
    let mut pb = PathBuf::new();
    pb.push(dir);
    pb.push(filename);
    Ok(pb)
}

pub fn path_to_lossy_string (path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().as_ref().to_string()
}

pub fn path_to_unchecked_string (path: impl AsRef<Path>) -> String {
    path.as_ref().to_str().unwrap().to_string()
}

pub fn readable_file (dir: &str, filename: &str) -> Result<File> {
    let p = filepath(dir,filename)?;
    if p.is_file() {
        File::open(p)
    } else {
        Err(io_error!(Other, "not a regular file {:?}", p))
    }
}

pub fn writable_empty_file (dir: impl AsRef<Path>, filename: &str) -> Result<File> {
    let path = dir.as_ref().join(filename);
    File::create( path)
}

pub fn file_contents_as_string (file: &mut fs::File) -> Result<String> {
    let len = file.metadata()?.len();
    let mut contents = String::with_capacity(len as usize);
    file.read_to_string(&mut contents)?;
    Ok(contents)
}

pub fn filepath_contents_as_string <P: AsRef<Path>> (path: &P) -> Result<String> {
    let mut file = File::open(path)?;
    file_contents_as_string( &mut file)
}

pub fn filepath_contents <P: AsRef<Path>> (path: &P) -> Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let md = file.metadata()?;
    let len = md.len();
    if len > 0 {
        let mut contents: Vec<u8> = Vec::with_capacity(len as usize);
        file.read_to_end(&mut contents)?;
        Ok(contents)

    } else { Err(io_error!(Other, "file empty: {:?}", file)) }
}

pub fn file_length <P: AsRef<Path>> (path: P) -> Option<u64> {
    fs::metadata(path).ok().map( |meta| meta.len() )
}

pub fn get_modified_timestamp <P: AsRef<Path>> (path: P) -> Option<SystemTime> {
    if let Some(meta) = fs::metadata(path).ok() {
        meta.modified().ok()
    } else {
        None
    }
}

pub fn set_modified_timestamp <P: AsRef<Path>> (path: P, t: SystemTime) -> Result<()> {
    let f = File::open(path)?;
    f.set_modified(t)
}

pub fn get_accessed_timestamp <P: AsRef<Path>> (path: P) -> Option<SystemTime> {
    if let Some(meta) = fs::metadata(path).ok() {
        meta.accessed().ok()
    } else {
        None
    }
}

pub fn set_accessed_timestamp <P: AsRef<Path>> (path: P, t: SystemTime) -> Result<()> {
    let f = File::open(path)?;
    let ftimes = FileTimes::new().set_accessed(t);
    f.set_times( ftimes)
}

pub fn set_accessed<P: AsRef<Path>> (path: &P)->Result<()> {
    set_accessed_timestamp(path, SystemTime::now())
}

pub fn existing_non_empty_file_from_path <P: AsRef<Path>> (path: P)-> Result<File> {
    let mut file = File::open(path)?;
    let md = file.metadata()?;
    let len = md.len();
    if len > 0 {
        Ok(file)
    } else { Err(io_error!(Other, "file empty: {:?}", file)) }
}

pub fn existing_non_empty_file (dir: &str, filename: &str) -> Result<fs::File> {
    let mut pb = PathBuf::new();
    pb.push(dir);
    pb.push(filename);

    existing_non_empty_file_from_path(&pb)
}

pub fn create_file_with_backup (dir: &str, filename: &str, ext: &str) -> Result<File> {
    let mut pb = PathBuf::new();
    pb.push(dir);
    pb.push(filename);
    let p = pb.as_path();

    if p.is_file() && p.metadata()?.len() > 0 {
        let mut pb_bak = pb.clone();
        pb_bak.push(ext);
        let p_bak = pb_bak.as_path();

        if p_bak.is_file() { fs::remove_file(p_bak)?; }
        fs::rename(p, p_bak)?;
    }

    File::create(p)
}

pub fn set_filepath_contents (dir: impl AsRef<Path>, filename: &str, new_contents: &[u8]) -> Result<()>  {
    let mut file = writable_empty_file(dir,filename)?;
    set_file_contents(&mut file, new_contents)
}

pub fn set_file_contents(file: &mut File, new_contents: &[u8]) -> Result<()> {
    file.write_all(new_contents)
}

pub fn set_filepath_contents_with_backup (dir: &str, filename: &str, backup_ext: &str, new_contents: &[u8]) -> Result<()> {
    let mut file = create_file_with_backup(dir,filename,backup_ext)?;
    set_file_contents(&mut file, new_contents)
}

pub fn append_open (path: impl AsRef<Path>)->Result<File> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .append(true)
        .open(path.as_ref())
}

pub fn append_to_file (path: impl AsRef<Path>, s: &str) -> Result<()> {
    let mut file = append_open( path.as_ref())?;
    write!( file, "{s}")
}

pub fn append_line_to_file (path: impl AsRef<Path>, s: &str) -> Result<()> {
    let mut file = append_open( path.as_ref())?;
    writeln!( file, "{s}")
}

pub fn get_filename_extension<'a> (path: &'a str) -> Option<&'a str> {
    if let Some(idx) = path.rfind('.') {
        if idx < path.len()-1 { 
            return Some( path[idx+1..].as_ref() )
        }
    }
    None

    //let path = Path::new(path);
    //path.extension().and_then( |ostr| ostr.to_str())
}

/// this is the non-extension part of a filename. Input can be a path - everything up to the last
/// path separator is discarded (on Windows we accept both '\\' and '/' as separator)
pub fn get_file_basename<'a> (path: &'a str) -> Option<&'a str> {
    let i0 = if let Some(idx) = path.rfind( std::path::MAIN_SEPARATOR) { 
        idx + 1 
    } else {
        if std::path::MAIN_SEPARATOR != '/' {
            if let Some(idx) = path.rfind('/') { idx + 1 } else { 0 }
        } else { 0 }
    };

    let i1 = if let Some(idx) = path.rfind('.') { idx } else { path.len() };

    if i1 > i0 {
        Some( path[i0..i1].as_ref() )
    } else {
        None
    }
}

pub fn basename<'a,P: AsRef<Path>> (path: &'a P)->Option<&'a str> {
    filename(path).and_then(|fname| get_file_basename(fname))
}

pub fn remove_old_files<T> (dir: &T, max_age: Duration)->Result<usize> where T: AsRef<Path> {
    let dir: &Path = dir.as_ref();

    if dir.is_dir() {
        let now = SystemTime::now(); // TODO - this should use sim clock but that means we also have to translate the filesystem modified timestamp
        let mut n_removed = 0;

        for e in fs::read_dir(dir)? {
            let e = e?;
            let path = e.path();
            if path.is_file() {
                let meta = fs::metadata(&path)?;
                if let Ok(last_mod) = meta.modified() {
                    if let Ok(age) = now.duration_since(last_mod) {
                        if age > max_age {
                            if fs::remove_file(&path).is_ok() { n_removed += 1 }
                        }
                    }
                }
            }
        }

        Ok(n_removed)
    } else {
        Err( io_error!(NotFound, "dir {:?}", dir))
    }
}

pub fn visit_dirs (dir: &Path, recursive: bool, cb: &mut dyn FnMut(&DirEntry)) -> io::Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() && recursive {
                visit_dirs(&path, recursive, cb)?;
            } else {
                cb(&entry);
            }
        }
    }
    Ok(())
}

pub fn matching_files_in_dir<P: AsRef<Path>> (dir: &P, fname_regex: &Regex) -> Result<Vec<PathBuf>> {
    let dir: &Path = dir.as_ref();
    let mut list: Vec<PathBuf> = Vec::new();

    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            if let Ok(entry) = entry {
                if let Some(fname) = entry.file_name().to_str() {
                    if fname_regex.is_match( fname) {
                        list.push(entry.path())
                    }
                }
            }
        }
    }

    Ok(list)
}

pub fn store_file_contents_in_dir<P: AsRef<Path>> (dir: &P, filename: &str, contents: &[u8]) -> Result<()> {
    let dir: &Path = dir.as_ref();

    let path = dir.join(filename);
    let mut file = File::create(path)?;
    file.write_all( contents);

    Ok(())
}

pub fn lru_files<P: AsRef<Path>> (dir: &P, recursive: bool) -> Result<Vec<(PathBuf,SystemTime,u64)>> {
    let mut acc: Vec<(PathBuf,SystemTime,u64)> = Vec::new();
    let mut cb = |entry: &DirEntry| {
        if let Ok(meta) = entry.metadata() {
            if let Ok(last_access) = meta.accessed() {
                let len = meta.len();

                for (i,e) in acc.iter_mut().enumerate() {
                    if last_access > e.1 {
                        acc.insert(i, (entry.path(), last_access, len));
                        return;
                    }
                }
                acc.push( (entry.path(), last_access, len));
            }
        }
    };
    visit_dirs( dir.as_ref(), recursive, &mut cb)?;

    Ok(acc)
}

pub fn purge_lru_files_above_limit<P: AsRef<Path>> (dir: &P, recursive: bool, max_size: u64) -> Result<()> {
    let lru = lru_files( dir, recursive)?;

    let mut size: u64 = 0;
    for e in &lru {
        size += e.2;
        if size > max_size {
            std::fs::remove_file(&e.0)?
        }
    }
    Ok(())
}

pub fn dir_size <P: AsRef<Path>> (dir: &P, recursive: bool) -> Result<u64> {
    let mut acc: u64 = 0;
    let mut cb = |entry: &DirEntry| {
        if let Ok(meta) = entry.metadata() {
            acc += meta.len();
        }
    };

    visit_dirs( dir.as_ref(), recursive,  &mut cb)?;
    Ok(acc)
}

/// check if total size of files in directory exceeds given bounds
/// if so, remove files with oldes accessed time until it does
pub fn lru_dir_bound <P: AsRef<Path>> (dir: &P, recursive: bool, max_size: u64) -> Result<bool> {
    let size = dir_size(dir, recursive)?;

    if size > max_size {
        let excess = size - max_size;
        let lru_entries = lru_files(dir, recursive)?;
        let mut removed: u64 = 0;
        for e in lru_entries.iter().rev() {
            fs::remove_file( &e.0)?;
            removed += e.2;
            if removed >= excess { return Ok(true) }
        }
        Err( io_error!(Other, "dir {:?} cannot be reduced to {} size", dir.as_ref(), max_size) ) // can't get here

    } else { Ok(false) } // nothing to shrink, we are under the limit
}

/// replace '~/' and '$<name>' path elements 
pub fn replace_env_var_path (path: impl AsRef<Path>)->Result<PathBuf> {
    let mut p = PathBuf::new();
    let mut n = 0;

    for c in path.as_ref().iter() {
        let e = c.to_str().ok_or( IOError::new(ErrorKind::Other, format!("invalid path: {:?}", path.as_ref())))?;

        if e == "~/" && n == 0 { 
            p.push( env::var("HOME").map_err( |err| IOError::new(ErrorKind::Other, "HOME not set"))?)
        } else if e.starts_with('$') {
            if e.len() > 1 {
                if e == "$ODIN_ROOT" { // this we set automatically
                    p.push( odin_build::root_dir())
                } else { // all others have to be set in the environment
                    p.push( env::var( e.get(1..).unwrap()).map_err( |err| IOError::new(ErrorKind::Other, format!("{e} not set")))?)
                }

            } else {
                p.push( e)
            }
        } else {
            p.push(e)
        }

        n += 1;
    }

    Ok(p.to_path_buf())
}

/// generic notification of file availability (can be used as a message)
#[derive(Debug,Clone)]
pub struct FileAvailable {
    pub topic: String,
    pub pathname: PathBuf,
}

/* #region EnvPathBuf *******************************************************************************/

use serde::ser::{Serialize as SerializeTrait, SerializeSeq, Serializer, SerializeStruct};
use serde::de::{Deserialize as DeserializeTrait, Deserializer};
use std::{fmt::Debug,ops::Deref,ffi::OsStr};

// a PathBuf that can use env vars as path elements
pub struct EnvPathBuf(PathBuf);

impl Debug for EnvPathBuf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl Deref for EnvPathBuf {
    type Target = PathBuf;

    fn deref (&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<Path> for EnvPathBuf {
    #[inline]
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<OsStr> for EnvPathBuf {
    #[inline]
    fn as_ref(&self) -> &OsStr {
        &self.0.as_ref()
    }
}

// note this means we serialize the expanded pathbug
impl SerializeTrait for EnvPathBuf {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> where S: Serializer {
        self.0.serialize( serializer)
    }
}

impl<'de> DeserializeTrait<'de> for EnvPathBuf {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error> where D: Deserializer<'de> {
        let a = String::deserialize(deserializer)?;
        let ep = PathBuf::from(a);
        let p = replace_env_var_path(&ep).map_err(|e| serde::de::Error::custom(&format!("failed to expand path {ep:?}")))?;

        Ok( EnvPathBuf(p) )
    }
}

/* #endregion EnvPathBuf */