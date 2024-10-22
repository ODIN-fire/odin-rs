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

use std::io;
use std::fs;
use std::io::{Read, Write};
use std::time::{SystemTime,Duration};
use io::ErrorKind::*;
use std::fs::{File,OpenOptions};
use std::path::{Path,PathBuf};
use std::io::{Error as IOError,ErrorKind};

use crate::macros::io_error;

type Result<T> = std::result::Result<T,std::io::Error>;

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

pub fn readable_file (dir: &str, filename: &str) -> Result<File> {
    let p = filepath(dir,filename)?;
    if p.is_file() {
        File::open(p)
    } else {
        Err(io_error!(Other, "not a regular file {:?}", p))
    }
}

pub fn writable_empty_file (dir: &str, filename: &str) -> Result<File> {
    File::create(filepath(dir,filename)?)
}

pub fn file_contents_as_string (file: &mut fs::File) -> Result<String> {
    let len = file.metadata()?.len();
    let mut contents = String::with_capacity(len as usize);
    file.read_to_string(&mut contents)?;
    Ok(contents)
}

pub fn filepath_contents_as_string (dir: &str, filename: &str) -> Result<String> {
    let mut file = readable_file(dir,filename)?;
    file_contents_as_string( &mut file)
}

pub fn file_length(file: &File) -> Result<u64> {
    file.metadata().and_then( |md| {
        if md.is_file() {
            Ok(md.len())
        } else {
            Err(io_error!(NotFound, "file {:?}", file))
        }
    })
}

pub fn existing_non_empty_file_from_path <P: AsRef<Path>> (path: P)-> Result<File> {
    match File::open(path) {
        Ok(file) => {
            let md = file.metadata()?;
            if md.is_file() {
                if md.len() > 0 {
                    Ok(file)
                } else {
                    Err(io_error!(Other, "file empty: {:?}", file))
                }
            } else {
                Err(io_error!(Other, "not a file: {:?}", file))
            }
        },
        Err(e) => Err(e)
    }
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

pub fn set_filepath_contents (dir: &str, filename: &str, new_contents: &[u8]) -> Result<()> {
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


pub fn remove_old_files<T> (dir: &T, max_age: Duration)->Result<usize> where T: AsRef<Path> {
    let dir: &Path = dir.as_ref();

    if dir.is_dir() {
        let now = SystemTime::now();
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


/// generic notification of file availability (can be used as a message)
#[derive(Debug,Clone)]
pub struct FileAvailable {
    pub topic: String,
    pub pathname: PathBuf,
}
