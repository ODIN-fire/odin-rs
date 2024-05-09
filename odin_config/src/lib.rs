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

//! the odin_config module serves two purposes: 
//! 
//!   (1) to handle mandatory config data lookup supporting different build options through the use of the [`config_for] macro, and 
//!   (2) to locate config/cache/data dirs to be used explicitly by the application/crate in a consistent way (e.g. according to the XDG spec)
//! 
//! [`config_for`] is using features to determine the kind of lookup that is performed, and is primarily used to either embed 
//! mandatory configuration data (encrypted or just compressed) within the application or to use XDG to locate respective
//! config sources.
//! 
//! ## Feature Flags
//! 
//! The following features are supported. Please note these require a respective `build.rs` build script in the root dir
//! of the target application
//! 
//!   (1) `config_local`: get config files from ODIN_LOCAL/config (or ./local/config/ if not defined)  
//!       no features need to be specified when buidling the application
//!       this is the default but should only be used for development
//! 
//!   (2) `config_xdg`: get config files from platform specific XDG directories
//!       build with `cargo build --features config_xdg ...`
//!       this is a production mode for machines in a controlled environment
//!       see https://specifications.freedesktop.org/basedir-spec/basedir-spec-latest.html for details
//! 
//!   (3) `config_embedded`: this generates a _config_data_ file with compressed bytes of the local config data
//!       which is then included within the target application
//!       build with `cargo build --features config_embedded ...`
//!       note that while this does not show cleartext config data in the executable binary the data is not encrypted
//! 
//!   (4) `config_embedded_pw`: generate a passphrase encrypted _config_data_ file to be included in the target application
//!       build with `ODIN_PP=... cargo build --features config_embedded_pw ...`
//!       note the provided passphrase is not stored anywhere. You loose it you can't run the target application.
//!       the default encryption uses aes256
//! 
//!   (5) `config_embedded_pgp`: generate PGP encrypted _config_data_
//!       this is the most secure mode. Users only have to provide their public PGP keys, i.e. the secret key does not
//!       have to be shared with the developer. At runtime, the target application needs access to the users
//!       private key and the user needs to enter the respective passphrase, i.e. this is 2FA
//!       build with `ODIN_KEY=<dir>/<usr-name> cargo build --features config_embedded_pgp ...`
//!       the ODIN_KEY env var is used as a prefix to derive the key file names (<dir>/<usr-name>_{private,public}.asc)

pub mod prelude;

pub mod errors;
use crate::errors::{OdinConfigError,ConfigResult};
type Result<T> = crate::errors::ConfigResult<T>;

pub mod app;
use crate::app::*;

pub mod pw_cache;
use crate::pw_cache::*;

#[cfg(feature="config_embedded")]
use miniz_oxide::inflate::decompress_to_vec;

#[cfg(feature="config_embedded_pgp")]
use pgp::MessageDecrypter;

use std::ffi::OsStr;
use std::{fs::File,path::Path,any::type_name,io::Read};
use ron::de::from_bytes;
use serde::Deserialize;

/// the config_for! macro variants - the main purpose of this crate.
/// use:
/// ```
/// use config::prelude::*;
/// ...
/// let my_config: MyConfig = config_for!(my_config)?;
/// ``` 
/// Note that we have to use attribute macros so that non-active variants are
/// removed (they wouldn't compile in case of the embedded_* options, which rely
/// on a build.rs creating the embedded config source).
/// 
/// Note also that users have to import the config prelude

#[macro_export]
#[cfg(feature="config_embedded")]
macro_rules! config_for {
    ($id:literal) => {
        odin_config::config_from_embedded( __c_data__($id).ok_or_else( || odin_config::errors::file_not_found($id))?)
    }
}

#[macro_export]
#[cfg(feature="config_embedded_pgp")]
macro_rules! config_for {
    ($id:literal) => {
        odin_config::config_from_embedded_pgp( &PW_CACHE, __c_data__($id).ok_or_else( || odin_config::errors::file_not_found($id))?)
    }
}

#[macro_export]
#[cfg(feature="config_embedded_pw")]
macro_rules! config_for {
    ($id:literal) => {
        odin_config::config_from_embedded_pw( &PW_CACHE, __c_data__($id).ok_or_else( || odin_config::errors::file_not_found($id))?)
    }
}

#[macro_export]
#[cfg(feature="config_xdg")]
macro_rules! config_for {
    ($id:literal) => {
        odin_config::config_from_xdg_file( &APP, $id)
    }
}

// default is relative 'local' dir
#[macro_export]
#[cfg(not(any(feature="config_embedded",feature="config_embedded_pgp",feature="config_embedded_pw",feature="config_xdg")))]
macro_rules! config_for {
    ($id:literal) => {
        odin_config::config_from_local_file( $id)
    }
}

//--- the use_config!() variants

#[macro_export]
#[cfg(feature="config_embedded")]
macro_rules! use_config {
    () => {
        include!(concat!(env!("OUT_DIR"), "/config_data"));
    };
}

#[macro_export]
#[cfg(any(feature="config_embedded_pgp",feature="config_embedded_pw"))]
macro_rules! use_config {
    () => {
        include!(concat!(env!("OUT_DIR"), "/config_data"));

        lazy_static::lazy_static! {
            static ref PW_CACHE: odin_config::pw_cache::PwCache = odin_config::pw_cache::PwCache::new("please enter key passphrase", std::time::Duration::from_secs(20));
        }
    };
}

#[macro_export]
#[cfg(not(any(feature="config_embedded",feature="config_embedded_pgp",feature="config_embedded_pw")))]
macro_rules! use_config {
    () => { }
}

lazy_static::lazy_static! {
    static ref APP: AppMetaData = AppMetaData::new();
}

pub fn app_metadata()->&'static AppMetaData { &APP }


/* #region config retrievers ******************************************************************************/

#[cfg(feature="config_embedded")]
pub fn config_from_embedded<C> (bs: &[u8])->Result<C>  where C: for <'a> Deserialize<'a> {
    let data = decompress_to_vec( bs)?;
    Ok(from_bytes(&data)?)
}

#[cfg(feature="config_xdg")]
pub fn config_from_xdg_file<C> (app: &AppMetaData, id: &str)->Result<C>   where C: for <'a> Deserialize<'a> {
    let pn = format!("{id}.ron");
    let path: &Path = Path::new( &pn);
    app.load_config(&path)
}

/// RON config lookup through ODIN_LOCAL env var or ./local directory as a fallback. The config pathname is constructed
/// from «local-root»/config/«id».ron
#[cfg(not(any(feature="config_embedded",feature="config_embedded_pgp",feature="config_xdg")))]
pub fn config_from_local_file<C> (id: &str)->Result<C>   where C: for <'a> Deserialize<'a> {
    use errors::file_not_found;

    let pn = format!("{}/config/{}.ron", get_local_dir(), id);
    let path: &Path = Path::new( &pn);
    if !path.is_file() { return Err(file_not_found(path.to_str().unwrap())) }

    let mut file = File::open(path)?;
    let len = file.metadata().unwrap().len();
    let mut data: Vec<u8> = Vec::with_capacity(len as usize);
    file.read_to_end(&mut data).unwrap();
    
    Ok(from_bytes(&data)?)
}

fn get_local_dir ()->String { 
    match std::env::var("ODIN_LOCAL") {
        Ok(local_root) => {
            if local_root.ends_with(std::path::MAIN_SEPARATOR) {
                if let Ok(cwd) = std::env::current_dir() {
                    if let Some(dir) = cwd.file_name() {
                        return format!("{local_root}{}", dir.to_string_lossy())
                    }
                }
            }
            local_root
        }
        _ => "./local".to_string()
    }
}

#[cfg(feature="config_embedded_pgp")]
pub fn config_from_embedded_pgp<C> (pw_cache: &pw_cache::PwCache, bs: &[u8])->Result<C>  where C: for <'a> Deserialize<'a> {
    use pgp::{Deserializable, composed::{signed_key::SignedSecretKey,message::Message}};
    use std::{io::Cursor, fs::File};
    use errors::config_error;

    let mut priv_key_filename = std::env::var("ODIN_KEY")?;
    if !priv_key_filename.ends_with("_private.asc") { priv_key_filename.push_str("_private.asc") }
    let mut priv_key_file = File::open(priv_key_filename)?;
    let (priv_key, _headers) = SignedSecretKey::from_armor_single(&mut priv_key_file)?;
    priv_key.verify()?;

    let parsed = Message::from_armor_single(Cursor::new(bs))?.0;

    // unfortunately Message only works with a String pw
    let (mut decryptor,keys) = pw_cache.with_string_pw( |pw| {
        parsed.decrypt(|| pw , &[&priv_key]).map_err(|e| OdinConfigError::PgpError(e))
    })?;
    let decrypted = decryptor.next().ok_or(config_error("invalid embedded PGP data"))??.decompress()?;

    if let Message::Literal(literal_data) = decrypted {
        Ok(from_bytes(literal_data.data())?)
    } else {
        Err(config_error("invalid embedded PGP data"))
    }
}

#[cfg(feature="config_embedded_pw")]
pub fn config_from_embedded_pw<C> (pw_cache: &pw_cache::PwCache, bs: &[u8])->Result<C>  where C: for <'a> Deserialize<'a> {
    let decrypted: Vec<u8> = pw_cache.with_u8_pw(|pw| {
        use magic_crypt::MagicCryptTrait;
        let mc = magic_crypt::new_magic_crypt!(pw,256);
        Ok(mc.decrypt_bytes_to_bytes(bs))
    })??;

    Ok(from_bytes(decrypted.as_slice())?)
}

/* #endregion config retrievers */

/// this is mostly for testing purposes and should not be used in production, which should be based on one of the
/// feature-gated config lookup options
pub fn load_config <C:serde::de::DeserializeOwned> (pathname: impl AsRef<OsStr>)->ConfigResult<C> {
    let path = Path::new(&pathname);
    if path.is_file() {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        let mut contents = String::with_capacity(len as usize);
        file.read_to_string(&mut contents)?;
        ron::from_str::<C>(contents.as_str()).map_err(|e| OdinConfigError::ConfigParseError(format!("{:?}", e)))
    } else {
        Err( OdinConfigError::ConfigFileNotFound(path.as_os_str().to_string_lossy().to_string()) )
    }
}