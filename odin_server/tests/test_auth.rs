/*
 * Copyright 2026 Peter Mehlitz
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *    http://apache.org
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */
#![cfg(feature="auth")]

use std::{net::SocketAddr, sync::{Arc, Mutex}, path::PathBuf};
use axum::{Router, body::Body, extract::connect_info::MockConnectInfo, http::{Request, StatusCode, header}, routing::get};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;
use odin_common::{auth::AuthConfig, fs::EnvPathBuf};
use odin_server::{ServerConfig, auth::{self, ServerAuth, RoleSet, is_same_origin}};

struct TestDir(PathBuf);
impl TestDir {
    fn new (name: &str)->Self {
        let path = std::env::temp_dir().join( format!("odin_server_test_{name}_{}", std::process::id()));
        std::fs::create_dir_all( &path).unwrap();
        TestDir(path)
    }
}
impl Drop for TestDir {
    fn drop (&mut self) { let _ = std::fs::remove_dir_all( &self.0); }
}

fn server_config (dir: &TestDir, sock_addr: &str, allow_insecure: bool)->ServerConfig {
    let mut auth = AuthConfig::with_roles( &["viewer", "editor"]);
    auth.methods = vec![ auth::AuthMethod::Password, auth::AuthMethod::Passkey ];
    auth.min_password_len = 8;
    auth.db_path = Some( EnvPathBuf( dir.0.join("auth.db")));
    auth.allow_insecure = allow_insecure;
    ServerConfig{ sock_addr: sock_addr.parse().unwrap(), tls: None, heartbeat_interval: None, auth: Some(auth) }
}

async fn send (router: &Router, req: Request<Body>)->(StatusCode, Option<String>, Value) {
    let res = router.clone().oneshot( req).await.unwrap();
    let status = res.status();
    let cookie = res.headers().get( header::SET_COOKIE).map( |v| v.to_str().unwrap().to_string());
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice( &bytes).unwrap_or( Value::Null);
    (status, cookie, json)
}

fn post (path: &str, body: Value, cookie: Option<&str>)->Request<Body> {
    let mut b = Request::post( path).header( header::CONTENT_TYPE, "application/json");
    if let Some(c) = cookie { b = b.header( header::COOKIE, c); }
    b.body( Body::from( body.to_string())).unwrap()
}

fn get_req (path: &str, cookie: Option<&str>)->Request<Body> {
    let mut b = Request::get( path);
    if let Some(c) = cookie { b = b.header( header::COOKIE, c); }
    b.body( Body::empty()).unwrap()
}

#[tokio::test]
async fn test_auth_routes () {
    let dir = TestDir::new("routes");
    let config = server_config( &dir, "127.0.0.1:9123", false);
    let auth = Arc::new( ServerAuth::new( &config, config.auth.as_ref().unwrap()).await.unwrap());
    assert_eq!( auth.cookie_name, "odin_session_9123");
    assert_eq!( auth.origins, vec!["http://localhost:9123".to_string()]);

    // admin side: create user and invite
    auth.authenticator.create_user( "alice", &["viewer"], None).await.unwrap();
    let (token, _) = auth.authenticator.create_invite( "alice").await.unwrap();

    let logged_out = Arc::new( Mutex::new( Vec::<Arc<str>>::new()));
    let lo = logged_out.clone();
    let editor = auth.role_map().role_set( &["editor"]).unwrap();
    let router = Router::new()
        .nest( "/app/auth", auth::auth_router( auth.clone(), Arc::new( move |s| lo.lock().unwrap().push(s))))
        .merge( auth::protect( Router::new().route( "/app/viewer", get( || async { "viewer data" })), auth.clone(), RoleSet::NONE))
        .merge( auth::protect( Router::new().route( "/app/editor", get( || async { "editor data" })), auth.clone(), editor))
        .layer( MockConnectInfo( SocketAddr::from( ([127,0,0,1], 4711))));

    //--- enrollment with invite token
    let (status, _, info) = send( &router, post( "/app/auth/enroll/info", json!({"token": token}), None)).await;
    assert_eq!( status, StatusCode::OK);
    assert_eq!( info["user"], "alice");
    assert_eq!( info["invite"], true);
    assert_eq!( info["methods"], json!(["password", "passkey"]));
    assert_eq!( info["rpId"], "localhost");

    let (status, _, _) = send( &router, post( "/app/auth/enroll/info", json!({"token": "bogus"}), None)).await;
    assert_eq!( status, StatusCode::FORBIDDEN);
    let (status, _, _) = send( &router, post( "/app/auth/enroll/info", json!({}), None)).await;
    assert_eq!( status, StatusCode::UNAUTHORIZED);

    let (status, _, err) = send( &router, post( "/app/auth/enroll/password", json!({"token": token, "password": "short"}), None)).await;
    assert_eq!( status, StatusCode::BAD_REQUEST);
    assert!( err["error"].as_str().unwrap().contains("at least 8"));

    let (status, cookie, _) = send( &router, post( "/app/auth/enroll/password", json!({"token": token, "password": "alice's secret"}), None)).await;
    assert_eq!( status, StatusCode::OK);
    let cookie = cookie.expect("session cookie");
    assert!( cookie.starts_with("odin_session_9123=") && cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));
    assert!( !cookie.contains("Secure")); // no TLS
    let session = cookie.split(';').next().unwrap().to_string();

    //--- session user and protected routes
    let (status, _, user) = send( &router, get_req( "/app/auth/user", Some(&session))).await;
    assert_eq!( status, StatusCode::OK);
    assert_eq!( user["name"], "alice");
    assert_eq!( user["roles"], json!(["viewer"]));

    assert_eq!( send( &router, get_req( "/app/viewer", Some(&session))).await.0, StatusCode::OK);
    assert_eq!( send( &router, get_req( "/app/editor", Some(&session))).await.0, StatusCode::FORBIDDEN);
    assert_eq!( send( &router, get_req( "/app/viewer", None)).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!( send( &router, get_req( "/app/viewer", Some("odin_session_9123=bogus"))).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!( send( &router, get_req( "/app/viewer", Some("other=1; odin_session_9124=x"))).await.0, StatusCode::UNAUTHORIZED);

    //--- password login
    let (status, _, err) = send( &router, post( "/app/auth/login/password", json!({"username": "alice", "password": "wrong"}), None)).await;
    assert_eq!( status, StatusCode::UNAUTHORIZED);
    assert_eq!( err["error"], "invalid credentials");
    let (status, cookie2, _) = send( &router, post( "/app/auth/login/password", json!({"username": "alice", "password": "alice's secret"}), None)).await;
    assert_eq!( status, StatusCode::OK);
    let session2 = cookie2.unwrap().split(';').next().unwrap().to_string();

    // forms can't post (no JSON content type)
    let form = Request::post( "/app/auth/login/password").header( header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body( Body::from("username=alice&password=x")).unwrap();
    assert_eq!( send( &router, form).await.0, StatusCode::UNSUPPORTED_MEDIA_TYPE);

    // methods that are not enabled
    let (status, _, _) = send( &router, post( "/app/auth/login/totp", json!({"username": "alice", "code": "123456"}), None)).await;
    assert_eq!( status, StatusCode::BAD_REQUEST);

    // passkey start for unknown user
    let (status, _, _) = send( &router, post( "/app/auth/login/passkey/start", json!({"username": "nobody"}), None)).await;
    assert_eq!( status, StatusCode::UNAUTHORIZED);

    //--- role change by admin is picked up after refresh
    auth.authenticator.set_roles( "alice", &["viewer", "editor"]).await.unwrap();
    auth.refresh().await;
    assert_eq!( send( &router, get_req( "/app/editor", Some(&session2))).await.0, StatusCode::OK);

    //--- logout
    let (status, cookie, _) = send( &router, post( "/app/auth/logout", json!({}), Some(&session))).await;
    assert_eq!( status, StatusCode::OK);
    assert!( cookie.unwrap().contains("Max-Age=0"));
    assert_eq!( logged_out.lock().unwrap().len(), 1);
    assert_eq!( send( &router, get_req( "/app/viewer", Some(&session))).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!( send( &router, get_req( "/app/viewer", Some(&session2))).await.0, StatusCode::OK); // other session still valid
}

#[tokio::test]
async fn test_insecure_config () {
    let dir = TestDir::new("insecure");
    let config = server_config( &dir, "0.0.0.0:9124", false);
    assert!( ServerAuth::new( &config, config.auth.as_ref().unwrap()).await.is_err()); // plain HTTP on non-loopback

    let config = server_config( &dir, "0.0.0.0:9124", true);
    assert!( ServerAuth::new( &config, config.auth.as_ref().unwrap()).await.is_ok());
}

#[test]
fn test_same_origin () {
    let req = |origin: Option<&str>, host: &str| {
        let mut b = Request::get("/app/ws").header( header::HOST, host);
        if let Some(o) = origin { b = b.header( header::ORIGIN, o); }
        b.body(()).unwrap()
    };
    assert!( is_same_origin( req( Some("http://localhost:9009"), "localhost:9009").headers()));
    assert!( is_same_origin( req( None, "localhost:9009").headers()));
    assert!( !is_same_origin( req( Some("https://evil.example.com"), "localhost:9009").headers()));
    assert!( !is_same_origin( req( Some("http://localhost:9010"), "localhost:9009").headers()));
}
