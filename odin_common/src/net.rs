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

///! common utility functions for network operations

use std::{collections::HashMap, fs::File, io::{self, Write}, net::{SocketAddr,IpAddr,Ipv4Addr}, path::Path, sync::Arc};
use reqwest::{header::{HeaderMap,HeaderName,HeaderValue,CONTENT_TYPE}, Client, IntoUrl, Request, Body, Response, StatusCode};
use regex::Regex;
use lazy_static::lazy_static;
use serde::{de::DeserializeOwned,Serialize,Deserialize};

use crate::{define_error, fs::{self, file_length, get_filename_extension, filepath_contents_as_string}, if_let};

const SCHEME: usize = 1;
const USR: usize = 2;
const HOST: usize = 3;
const PORT: usize = 4;
const PATH: usize = 5;
const QUERY: usize = 6;

lazy_static! {
    // [scheme,user,host,port,path,query]
    static ref URL_RE: Regex = Regex::new( r"(.+)://(?:(.+)@)?([^:/]+)(?::(\d+))?(?:/([^?]+))?(?:\?(.+))?").unwrap();
    static ref FNAME_RE: Regex = Regex::new( r"(?:.*/)(.*)").unwrap();
}

define_error!{ pub OdinNetError =
    IOError(#[from] std::io::Error) : "IO error: {0}",
    NotFoundError(String) : "not found {0}",
    HttpError(#[from] reqwest::Error) : "http error: {0}",
    OpFailed(String) : "operation failed: {0}",
    ParseError(String) : "parse error: {0}",
    WsError(String) : "websocket error: {0}"
}

pub type Result<T> = std::result::Result<T, OdinNetError>;

pub fn get_header_key_values (headers: &[String]) -> Result<Vec<(String,String)>> {
    Ok( headers.iter().flat_map( |s| {
        let s = s.as_str();
        s.find(':').map( |i| (s[0..i].to_string(), s[i+1..].to_string()))
    }).collect() )
}

pub fn get_headermap (headers: &Vec<String>) -> Result<HeaderMap> {
    if headers.is_empty() {
        Ok(HeaderMap::new())
    } else {
        let mut hm = HeaderMap::new();
        for h in headers {
            if let Some(idx) = h.find(':') {
                let k = h[0..idx].trim();
                let v = h[idx+1..].trim();
                hm.append(
                    HeaderName::from_bytes( k.as_bytes()).map_err(|e| OdinNetError::OpFailed(e.to_string()))?,
                    HeaderValue::from_str(v).map_err(|e| OdinNetError::OpFailed(e.to_string()))?
                );
            }
        }
        Ok(hm)
    }
}

pub fn header (key: &'static str, val: &'static str)-> (HeaderName,HeaderValue) {
    (HeaderName::from_static(key), HeaderValue::from_static(val))
}

pub const NO_HEADERS: Option<&Vec<(String,String)>> = None;

/// fetch file from URL using HTTP GET method. Retrieve in chunks to support large files
/// Note this requires a full URL
pub async fn get_file<'a,I> (client: &Client, url: &str, opt_headers: Option<I>, dir: impl AsRef<Path>) -> Result<u64>
    where I: IntoIterator<Item=&'a (String,String)>
{
    if let Some(fname) = url_file_name( url) {
        let path = dir.as_ref().to_path_buf().join(fname);
        download_url( client, url, opt_headers, &path).await
    } else {
        Err( OdinNetError::OpFailed(format!("not a file URL: {}", url)) )
    }
}

pub async fn get_differing_size_file<'a,I> (client: &Client, url: &str, opt_headers: Option<I>, dir: &str) -> Result<u64>
    where I: IntoIterator<Item=&'a (String,String)> + Clone
{
    if let Some(fname) = url_file_name( url) {
        let path = Path::new( dir).join(fname);

        if let Some(local_len) = file_length(&path)
        && let Ok(remote_len) = get_content_length( client, url, opt_headers.clone()).await {
            if local_len == remote_len {
                return Ok(local_len) // we assume equal length means same content
            }
        }

        // if we get here there either was no local file or it has a differing content size so we retrieve and overwrite
        download_url( client, url, opt_headers, &path).await

    } else {
        Err( OdinNetError::OpFailed(format!("not a file URL: {}", url)) )
    }
}

pub async fn post_query<'a,I> (client: &Client, url: &str, headers: Option<I>, path: impl AsRef<Path>, request_body: String) -> Result<u64>
    where I: IntoIterator<Item=&'a (HeaderName,HeaderValue)>
{
    let mut file = File::create(path)?;
    let mut len: u64 = 0;

    let mut req = client.post(url);

    if let Some(headers) = headers {
        for (key,value) in headers {
            req = req.header( key, value);
        }
    }

    let mut response = req
        .body( request_body)
        .send().await?;

    match response.status() {
        StatusCode::OK => {
            while let Some(chunk) = response.chunk().await? {
                len += chunk.len() as u64;
                file.write_all(&chunk)?;
            }

            file.flush()?;
            Ok(len)
        }
        StatusCode::NOT_FOUND => {
            Err( OdinNetError::NotFoundError(format!("{url}")))
        }
        other => {
            Err( OdinNetError::OpFailed(format!("response status {other:?}")))
        }
    }
}

pub async fn post_json_query<T,U> (client: &Client, url: &str, data: T) -> Result<U> where T: Serialize, U: for<'a> Deserialize<'a> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

    let req = client.post( url)
        .headers( headers)
        .json( &data);

    let mut response = req.send().await?;

    match response.status() {
        StatusCode::OK => {
            Ok( response.json().await? )
        }
        StatusCode::NOT_FOUND => {
            Err( OdinNetError::NotFoundError(format!("{url}")))
        }
        other => {
            Err( OdinNetError::OpFailed(format!("response status {other:?}")))
        }
    }
}

pub async fn download_url<'a,I> (client: &Client, url: &str, headers: Option<I>, path: impl AsRef<Path>) -> Result<u64>
    where I: IntoIterator<Item=&'a (String,String)>
{
    let mut file = File::create(path)?;
    let mut len: u64 = 0;

    let mut req = client.get(url);

    if let Some(headers) = headers {
        for (key,value) in headers {
            req = req.header( key, value);
        }
    }

    let mut response = req.send().await?;

    match response.status() {
        StatusCode::OK => {
            while let Some(chunk) = response.chunk().await? {
                len += chunk.len() as u64;
                file.write_all(&chunk)?;
            }

            file.flush()?;
            Ok(len)
        }
        StatusCode::NOT_FOUND => {
            Err( OdinNetError::NotFoundError(format!("{url}")))
        }
        other => {
            Err( OdinNetError::OpFailed(format!("response status {other:?}")))
        }
    }
}

/// get content-length of URL without retrieving the actual content
pub async fn get_content_length<'a,I> (client: &Client, url: &str, opt_headers: Option<I>)->Result<u64>
    where I: IntoIterator<Item=&'a (String,String)>
{
    let mut req = client.head(url);

    if let Some(headers) = opt_headers {
        for (key,value) in headers {
            req = req.header(key, value);
        }
    }

    let response = req.send().await?;

    let headers = response.headers();
    if let Some(content_length) = headers.get("content-length") {
        content_length.to_str()
            .map_err(|e| OdinNetError::OpFailed("invalid header value".into()))?
            .parse()
            .map_err(|e| OdinNetError::OpFailed("invalid content-length".into()))
    } else {
        Err( OdinNetError::OpFailed("no content-length".into()))
    }
}


/// get filename part (last path element) of complete URL
/// NOTE - this does not work for partial (relative) URLs
pub fn url_file_name<'a> (url: &'a str) -> Option<&'a str> {
    URL_RE.captures( url)
    .and_then( |cap| cap.get( PATH))
    .map( |m| m.as_str())
    .and_then( |p| FNAME_RE.captures( p))
    .and_then( |cap| cap.get(1))
    .map( |m| m.as_str())
}

/// Note - we assume lower case extensions without '.'
pub fn mime_type_for_extension (ext: &str) -> Option<&'static str> {
    MIME_MAP.get(ext).map(|v| &**v)
}

// Note - we assume lower case extensions
pub fn mime_type_for_path<'a,T: AsRef<Path>> (path: &'a T) -> Option<&'static str> {
    fs::extension(path).and_then(|ext| mime_type_for_extension(ext))
}

lazy_static! {
    static ref MIME_MAP: HashMap<&'static str, &'static str> = HashMap::from( [ // file extension -> mime type
        ("tif", "image/tiff"),
        ("tiff", "image/tiff"),
        ("png", "image/png"),
        ("jpg", "image/jpeg"),
        ("jpeg", "image/jpeg"),
        ("gif", "image/gif"),
        ("svg", "image/svg+xml"),
        ("webp", "image/webp"),
        ("webm", "video/webm"),
        ("weba", "audio/webm"),
        ("mpeg", "video/mpeg"),
        ("ts", "video/mp2t/"),
        ("mp3", "audio/mp3"),
        ("mp4", "video/mp4"),
        ("js", "text/javascript"),
        ("mjs", "text/javascript"),
        ("json", "application/json"),
        ("jsonld", "application/ld+json"),
        ("pdf", "application/pdf"),
        ("csv", "text/csv"),
        ("txt", "text/plain"),
        ("htm", "text/html"),
        ("html", "text/html"),
        ("css", "text/css"),
        ("gz", "application/gzip"),
        ("bz", "application/x-bzip"),
        ("bz2", "application/x-bzip2"),
        ("zip", "application/zip"),
        ("xml", "application/xml"),
        ("xhtml", "application/xhtml+xml")

        // and many more to follow..
    ]);
}

lazy_static! {
    pub static ref ENC_MAP: HashMap<&'static str, &'static str> = HashMap::from( [ // file extension -> content-encoding value
        ("gz", "gzip"),
        ("br", "br"),

        // and more to follow..
    ]);
}

pub fn enc_for (path: &str)->Option<&'static str> {
    let path_ext = get_filename_extension(path);
    for (ext,enc) in ENC_MAP.iter() {
        if path_ext == Some(*ext) {
            return Some(enc)
        }
    }
    None
}

pub const ZERO_ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);

pub fn encoding_for_extension (ext: &str) -> Option<&'static str> {
    ENC_MAP.get(ext).map(|v| &**v)
}

pub fn encoding_for_path<'a,T: AsRef<Path>> (path: &'a T) -> Option<&'static str> {
    fs::extension(path).and_then(|ext| encoding_for_extension(ext))
}

pub async fn from_json<T> (response: Response)->Result<T> where T: DeserializeOwned {
    let bytes = response.bytes().await?;
    //println!("@@ {}",String::from_utf8(bytes.to_vec()).unwrap());
    serde_json::from_slice( &bytes).map_err(|e| {
        //println!("@@ {}",String::from_utf8(bytes.to_vec()).unwrap());
        OdinNetError::ParseError(e.to_string())
    })
}

pub async fn get_deserialized_json_response<T> (url: &str)->Result<T>
    where T: DeserializeOwned
{
    let body = reqwest::get(url).await?.text().await?;
    Ok ( serde_json::from_str( &body).map_err(|e| OdinNetError::ParseError(e.to_string()))? )
}

pub async fn get_deserialized_cached_json_response<T,P> (url: &str, path: P)->Result<T>
    where T: DeserializeOwned, P: AsRef<Path>
{
    if path.as_ref().is_file() {
        let data = filepath_contents_as_string(&path)?;
        Ok ( serde_json::from_str( &data).map_err(|e| OdinNetError::ParseError(e.to_string()))? )

    } else {
        let body = reqwest::get(url).await?.text().await?;
        std::fs::write( path, &body)?;
        Ok ( serde_json::from_str( &body).map_err(|e| OdinNetError::ParseError(e.to_string()))? )
    }
}

/* #region ServerAddr ***************************************************************************************************/

/// a server (listen) address that is specified as `<host>:<port>`, with `<host>` being an IP address
/// (`127.0.0.1:9009`, `[::1]:9009`), a DNS name (`odin.example.com:443`) or a local network (mDNS/bonjour) name
/// (`beast.local:9009`). Names are resolved by the system resolver when the address is created (e.g. when a config is
/// deserialized). Note that resolution of `.local` names on Linux requires an mDNS resolver (e.g. avahi with nss-mdns).
///
/// A name can resolve to several addresses - the name of the local machine (e.g. `beast.local` on macOS) typically
/// resolves to the loopback address and the addresses of all network interfaces. Since clients can use any of them a
/// server should listen on all [`bind_addrs`](Self::bind_addrs) (all resolved addresses except of IPv6 link-local ones,
/// which would require interface scopes in URLs).
///
/// The primary socket address (the first non-loopback IPv4 address, if any) is accessible through `Deref`, i.e.
/// `addr.ip()` and `addr.port()` work as with [`SocketAddr`]. The [`host`](Self::host) is kept for server URLs, which
/// should use names if they were configured (e.g. for TLS certificates and passkeys). Serialization uses the original
/// `<host>:<port>` spec.
#[derive(Debug,Clone,PartialEq,Eq)]
pub struct ServerAddr {
    host: String,               // as specified (IPv6 addresses without brackets)
    addr: SocketAddr,           // primary resolved address
    bind_addrs: Vec<SocketAddr> // all addresses we listen on (includes addr)
}

impl ServerAddr {
    /// parse and (if required) resolve a `<host>:<port>` spec
    pub fn resolve (spec: &str)->Result<Self> {
        use std::net::ToSocketAddrs;
        let spec = spec.trim();

        if let Ok(addr) = spec.parse::<SocketAddr>() { // plain IP address, no resolution required
            return Ok( ServerAddr { host: addr.ip().to_string(), addr, bind_addrs: vec![addr] })
        }

        let (host, port) = spec.rsplit_once(':').ok_or_else( || OdinNetError::ParseError( format!("server address without port: '{spec}'")))?;
        let host = host.trim_start_matches('[').trim_end_matches(']');
        if host.is_empty() { return Err( OdinNetError::ParseError( format!("server address without host: '{spec}'"))) }
        let port: u16 = port.parse().map_err( |_| OdinNetError::ParseError( format!("invalid server port in '{spec}'")))?;

        let mut bind_addrs: Vec<SocketAddr> = Vec::new();
        for a in (host, port).to_socket_addrs().map_err( |e| OdinNetError::NotFoundError( format!("failed to resolve server host '{host}': {e}")))? {
            let is_link_local_v6 = match a.ip() { IpAddr::V6(ip) => ip.is_unicast_link_local(), _ => false };
            if !is_link_local_v6 && !bind_addrs.contains( &a) { bind_addrs.push( a) }
        }
        let addr = bind_addrs.iter().find( |a| a.is_ipv4() && !a.ip().is_loopback())
            .or_else( || bind_addrs.iter().find( |a| a.is_ipv4()))
            .or( bind_addrs.first()).cloned()
            .ok_or_else( || OdinNetError::NotFoundError( format!("no usable address for server host '{host}'")))?;

        Ok( ServerAddr { host: host.to_string(), addr, bind_addrs })
    }

    /// the host as specified (name or IP address)
    pub fn host (&self)->&str { &self.host }

    /// the primary resolved socket address
    pub fn socket_addr (&self)->SocketAddr { self.addr }

    /// all resolved socket addresses a server should listen on
    pub fn bind_addrs (&self)->&[SocketAddr] { &self.bind_addrs }

    /// is this only reachable through loopback addresses
    pub fn is_loopback (&self)->bool { self.bind_addrs.iter().all( |a| a.ip().is_loopback()) }

    /// was the host specified as a name (as opposed to an IP address)
    pub fn is_named (&self)->bool { self.host.parse::<IpAddr>().is_err() }

    /// host part for URLs (IPv6 addresses in brackets)
    pub fn url_host (&self)->String {
        if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() }
    }

    /// `<host>:<port>` as used in URLs
    pub fn authority (&self)->String { format!("{}:{}", self.url_host(), self.addr.port()) }
}

impl std::ops::Deref for ServerAddr {
    type Target = SocketAddr;
    fn deref (&self)->&SocketAddr { &self.addr }
}

impl From<SocketAddr> for ServerAddr {
    fn from (addr: SocketAddr)->Self { ServerAddr { host: addr.ip().to_string(), addr, bind_addrs: vec![addr] } }
}

impl std::str::FromStr for ServerAddr {
    type Err = OdinNetError;
    fn from_str (s: &str)->Result<Self> { ServerAddr::resolve(s) }
}

impl std::fmt::Display for ServerAddr {
    fn fmt (&self, f: &mut std::fmt::Formatter<'_>)->std::fmt::Result {
        if self.is_named() {
            let addrs: Vec<String> = self.bind_addrs.iter().map( |a| a.to_string()).collect();
            write!( f, "{} ({})", self.authority(), addrs.join(", "))
        } else {
            write!( f, "{}", self.addr)
        }
    }
}

impl Serialize for ServerAddr {
    fn serialize<S: serde::Serializer> (&self, serializer: S)->std::result::Result<S::Ok,S::Error> {
        serializer.serialize_str( &self.authority())
    }
}

impl<'de> Deserialize<'de> for ServerAddr {
    fn deserialize<D: serde::Deserializer<'de>> (deserializer: D)->std::result::Result<Self,D::Error> {
        let spec = String::deserialize( deserializer)?;
        ServerAddr::resolve( &spec).map_err( serde::de::Error::custom)
    }
}

/* #endregion ServerAddr */
