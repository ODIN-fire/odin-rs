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
//#![feature(diagnostic_namespace)]

use std::{net::SocketAddr, path::PathBuf};

use axum::{body::Body, response::Response, Router};
use axum_server::{service::MakeService, tls_rustls::RustlsConfig};

use reqwest::StatusCode;
use bytes::Bytes;

use serde::{Deserialize,Serialize};
use tokio::task::JoinHandle;

use odin_build::prelude::*;
use odin_common::strings;

pub mod prelude;
pub mod spa;
pub mod ui_service;
pub mod ws_service;

pub mod errors;
use errors::{OdinServerResult,op_failed};

define_load_config!{}
define_load_asset!{}

type Result<T> = OdinServerResult<T>;

#[derive(Deserialize,Serialize,Debug)]
pub struct ServerConfig {
    pub sock_addr: SocketAddr,
    pub tls: Option<TlsConfig>, // if set use TLS (https)
}

#[derive(Deserialize,Serialize,Debug)]
pub struct TlsConfig {
    pub cert_path: String, // path to PEM encoded certificate
    pub key_path: String,  // path to PEM encoded key data
}

/// get `Response` for given asset
/// NOTE - this has to be kept in sync with `odin_build` compression (which happens automatically)
pub fn get_asset_response (pathname: &str, bytes: Bytes) -> Response<Body> {
    let content_spec = odin_build::get_content_spec(pathname);
    build_ok_response( &content_spec.mime_type, content_spec.encoding, bytes)
}

fn build_ok_response (content_type: &str, encoding: Option<&str>, bytes: Bytes)->Response<Body> {
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", content_type);
 
    if let Some(enc) = encoding {
        builder = builder.header("Content-Encoding", enc);
    }

    builder.body( Body::from(bytes)).unwrap()
}

pub fn spawn_server_task (config: &ServerConfig, name: &str, router: Router) -> JoinHandle<()> {
    let sock_addr = config.sock_addr.clone();
    let router_svc = router.into_make_service_with_connect_info::<SocketAddr>();

    if let Some(tls) = &config.tls {
        println!("serving https://{}/{}", sock_addr, name);
        let cert_path = strings::env_expand( &tls.cert_path);
        let key_path = strings::env_expand( &tls.key_path);
        tokio::spawn( async move {
            let tls_config = RustlsConfig::from_pem_file(PathBuf::from(cert_path), PathBuf::from(key_path)).await.unwrap();
            axum_server::bind_rustls( sock_addr, tls_config).serve( router_svc).await.unwrap();
        })
    } else {
        println!("serving http://{}/{}", sock_addr, name);
        tokio::spawn( async move {
            let listener = tokio::net::TcpListener::bind(sock_addr).await.unwrap();
            axum::serve( listener, router_svc).await.unwrap();    
        })
    }
}


//--- syntactic sugar macros

#[macro_export]
macro_rules! asset_uri {
    ($fname:literal) => {
        concat!("./asset/", env!("CARGO_PKG_NAME"), "/", $fname)
    };
    ($crate_name:ident, $fname:literal) => {
        concat!("./asset/", stringify!($crate_name), "/", $fname)
    }
}

#[macro_export]
macro_rules! proxy_uri {
    ($pname:literal, $rel_uri:literal) => {
        concat!( "./proxy/", $pname, "/", $rel_uri);
    }
}

#[macro_export]
macro_rules! self_crate {
    () => { env!("CARGO_PKG_NAME") }
}

#[macro_export]
macro_rules! js_module_path {
    ($mod_name:literal) => {
        concat!( self_crate!(), "/", $mod_name)
    }
}

#[macro_export]
macro_rules! build_service {
    ( $($v:ident $(. $op:ident ())?),* => $e:expr) => {
        {
            $( let $v = $v $( .$op() )?; )*
            move || { $e }
        }
    };

    ( $e:expr) => {
        move || { $e }
    }
}
