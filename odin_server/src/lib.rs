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

use axum::{body::Body, response::Response};
use odin_build::prelude::*;

pub mod prelude;
pub mod spa;
pub mod ui_service;
pub mod ws_service;

pub mod errors;
use errors::{OdinServerResult,op_failed};
use reqwest::StatusCode;
use bytes::Bytes;

define_load_config!{}
define_load_asset!{}

/// get `Response` for given asset
/// NOTE - this has to be kept in sync with `odin_build` compression (which happens automatically)
pub fn get_asset_response (pathname: &str, bytes: Bytes) -> Response<Body> {
    match odin_build::extension(pathname) {
        Some("js")    => build_ok_response( "text/javascript", Some("br"), bytes),
        Some("css")   => build_ok_response( "text/css", Some("br"), bytes),
        Some("html")  => build_ok_response( "text/html", Some("br"), bytes),
        Some("json")  => build_ok_response( "application/json", Some("br"), bytes),
        Some("svg")   => build_ok_response( "image/svg+xml", Some("br"), bytes),

        Some("xml")   => build_ok_response( "application/xml", Some("br"), bytes),
        Some("csv")   => build_ok_response( "text/csv", Some("br"), bytes),
        Some("txt")   => build_ok_response( "text/plain", Some("br"), bytes),

        Some("jpeg")  => build_ok_response( "image/jpeg", None, bytes),
        Some("png")   => build_ok_response( "image/png", None, bytes),
        Some("webp")  => build_ok_response( "image/webp", None, bytes),
        Some("tif")   => build_ok_response( "image/tif", None, bytes),

        Some("mp4")   => build_ok_response( "video/mp4", None, bytes),
        Some("mpeg")  => build_ok_response( "video/mpeg", None, bytes),
        Some("webm")  => build_ok_response( "video/webm", None, bytes),
        Some("weba")  => build_ok_response( "audio/weba", None, bytes),

        _ => Response::builder()
            .status( StatusCode::BAD_REQUEST)
            .body( Body::from(format!("unsupported asset type: {}", pathname)))
            .unwrap()
    }
}

fn build_ok_response( content_type: &str, encoding: Option<&str>, bytes: Bytes)->Response<Body> {
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", content_type);
 
    if let Some(enc) = encoding {
        builder = builder.header("Content-Encoding", enc);
    }

    builder.body( Body::from(bytes)).unwrap()
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
