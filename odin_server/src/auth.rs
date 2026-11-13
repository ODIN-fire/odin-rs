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

//! axum integration of the server type independent [`odin_common::auth`] module: session cookies, the JSON routes
//! for login/enrollment/logout/user info, a middleware to protect routes with roles and websocket origin checks.
//! This is used by [`crate::spa::SpaServer`] but can also be used by other axum based servers.
//!
//! Routes (relative to the nesting path, e.g. `/<app>/auth`):
//!
//! | route                        | purpose |
//! |------------------------------|---------|
//! | `POST login/password`        | `{username,password}` -> session cookie |
//! | `POST login/totp`            | `{username,code}` -> session cookie |
//! | `POST login/passkey/start`   | `{username}` -> `{ceremony,options}` (for `navigator.credentials.get()`) |
//! | `POST login/passkey/finish`  | `{ceremony,credential}` -> session cookie |
//! | `POST enroll/info`           | `{token?}` -> `{user,methods}` (invite token or logged in user) |
//! | `POST enroll/passkey/start`  | `{token?,label?}` -> `{ceremony,options}` (for `navigator.credentials.create()`) |
//! | `POST enroll/passkey/finish` | `{ceremony,credential}` -> ok (+ session cookie if invite) |
//! | `POST enroll/totp/start`     | `{token?}` -> `{ceremony,url,qrPng,secret}` |
//! | `POST enroll/totp/finish`    | `{ceremony,code,label?}` -> ok (+ session cookie if invite) |
//! | `POST enroll/password`       | `{token?,password}` -> ok (+ session cookie if invite) |
//! | `POST logout`                | end session, clear cookie |
//! | `GET user`                   | `{name,roles}` of the session user (401 if not authenticated) |
//!
//! POST requests have to use `Content-Type: application/json`, which together with `SameSite=Strict` session cookies
//! protects against cross site request forgery.

pub use odin_common::auth::{AuthConfig, AuthMethod, AuthError, ConnUser, RoleSet, RoleMap, MsgAcl, is_authorized};

#[cfg(feature="auth")]
pub use imp::*;

#[cfg(not(feature="auth"))]
pub use stub::*;

/// check if a websocket upgrade (or other) request comes from the same origin as the server (protection against
/// cross site websocket hijacking with cookie based authentication). Requests without `Origin` header (non-browser
/// clients) are accepted - they still need a valid session
pub fn is_same_origin (headers: &http::HeaderMap)->bool {
    let Some(origin) = headers.get( http::header::ORIGIN).and_then( |v| v.to_str().ok()) else { return true };
    let Some(host) = headers.get( http::header::HOST).and_then( |v| v.to_str().ok()) else { return false };
    match origin.split_once("://") {
        Some((_, authority)) => authority.trim_end_matches('/') == host,
        None => false
    }
}

#[cfg(feature="auth")]
mod imp {
    use std::{net::SocketAddr, sync::Arc, collections::HashMap};
    use axum::{
        Json, Router,
        extract::{ConnectInfo, Request, State},
        http::{HeaderMap, HeaderValue, StatusCode, header},
        middleware::{self, Next},
        response::{IntoResponse, Response},
        routing::{get, post},
    };
    use serde::Deserialize;
    use serde_json::{json, Value};
    use odin_common::auth::*;
    pub use odin_common::auth::{Revalidation, LoginResult};
    use crate::{ServerConfig, errors::{OdinServerResult, op_failed}};

    /// the authentication context of an axum server
    pub struct ServerAuth {
        pub authenticator: Authenticator,
        pub cookie_name: String,
        pub secure_cookie: bool,
        pub origins: Vec<String>,
    }

    impl ServerAuth {
        /// create the auth context for a server. This fails if authentication would be done over plain HTTP on a
        /// non-loopback address (unless `allow_insecure` is set) or if the auth config is inconsistent
        pub async fn new (server_config: &ServerConfig, auth_config: &AuthConfig)->OdinServerResult<Self> {
            let sock_addr = server_config.sock_addr;
            let is_tls = server_config.tls.is_some();
            if !is_tls && !sock_addr.ip().is_loopback() && !auth_config.allow_insecure {
                return Err( op_failed( format!("authentication over plain HTTP on non-loopback address {sock_addr} requires TLS or 'allow_insecure: true'")))
            }
            let origins = server_origins( server_config, auth_config);
            let authenticator = Authenticator::new( auth_config.clone(), &origins).await.map_err( |e| op_failed( e.to_string()))?;

            Ok( ServerAuth{ authenticator, cookie_name: format!("odin_session_{}", sock_addr.port()), secure_cookie: is_tls, origins })
        }

        pub fn config (&self)->&AuthConfig { self.authenticator.config() }
        pub fn role_map (&self)->&RoleMap { self.authenticator.role_map() }

        /// reload users and purge expired data (call periodically, e.g. from a heartbeat timer)
        pub async fn refresh (&self) {
            if let Err(e) = self.authenticator.refresh().await { eprintln!("auth refresh failed: {e}"); }
        }

        /// check the session of an established connection (call after [`Self::refresh`])
        pub fn revalidate (&self, user: &ConnUser)->Revalidation { self.authenticator.revalidate( user) }

        /// the session token of a request (from our session cookie)
        pub fn session_token<'a> (&self, headers: &'a HeaderMap)->Option<&'a str> {
            for v in headers.get_all( header::COOKIE) {
                let Ok(s) = v.to_str() else { continue };
                for c in s.split(';') {
                    if let Some((k, v)) = c.trim().split_once('=') {
                        if k == self.cookie_name { return Some(v) }
                    }
                }
            }
            None
        }

        /// the authenticated user of a request (None if there is no valid session)
        pub async fn request_user (&self, headers: &HeaderMap)->Option<Arc<ConnUser>> {
            match self.session_token( headers) {
                Some(token) => self.authenticator.session_user( token).await,
                None => None
            }
        }

        fn session_cookie (&self, login: &LoginResult)->HeaderValue {
            let max_age = (login.expires - now_secs()).max(0);
            let secure = if self.secure_cookie { "; Secure" } else { "" };
            HeaderValue::from_str( &format!("{}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}", self.cookie_name, login.token)).unwrap()
        }

        fn clear_cookie (&self)->HeaderValue {
            let secure = if self.secure_cookie { "; Secure" } else { "" };
            HeaderValue::from_str( &format!("{}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{secure}", self.cookie_name)).unwrap()
        }

        /// the client side configuration for login/enroll documents (JSON)
        pub fn client_config (&self)->Value {
            json!({
                "methods": self.authenticator.available_methods().iter().map( |m| m.as_str()).collect::<Vec<_>>(),
                "rpId": self.config().rp_id,
                "minPasswordLength": self.config().min_password_len,
            })
        }

        /// the role set for a protected route `key`, using the `route_roles` config or `default_roles`
        pub fn route_roles (&self, key: &str, default_roles: &[String])->OdinServerResult<RoleSet> {
            let names = self.config().route_roles.get( key).map( |v| v.as_slice()).unwrap_or( default_roles);
            self.role_map().role_set( names).map_err( |e| op_failed( format!("protected route '{key}': {e}")))
        }

        /// build the ACL for incoming websocket messages from service defaults and config overrides (config wins).
        /// Unknown role names are an error
        pub fn ws_msg_acl (&self, service_defaults: &[(String,Vec<String>)])->OdinServerResult<MsgAcl> {
            self.build_acl( service_defaults, &self.config().ws_msg_roles, "ws_msg_roles")
        }

        /// build the ACL for services and outgoing websocket messages from service defaults (`SpaService::service_roles()`)
        /// and config overrides (config wins). Unknown role names are an error
        pub fn service_acl (&self, service_defaults: &[(String,Vec<String>)])->OdinServerResult<MsgAcl> {
            self.build_acl( service_defaults, &self.config().service_roles, "service_roles")
        }

        fn build_acl (&self, defaults: &[(String,Vec<String>)], overrides: &HashMap<String,Vec<String>>, what: &str)->OdinServerResult<MsgAcl> {
            let mut acl = MsgAcl::new();
            for (key, names) in defaults {
                if !overrides.contains_key( key) {
                    acl.set( key, self.role_map().role_set( names).map_err( |e| op_failed( format!("'{key}': {e} (define role or override in {what})")))?);
                }
            }
            acl.set_from_names( overrides, self.role_map()).map_err( |e| op_failed( format!("{what}: {e}")))?;
            Ok(acl)
        }
    }

    /// the origins under which the server is reachable for passkey ceremonies
    fn server_origins (server_config: &ServerConfig, auth_config: &AuthConfig)->Vec<String> {
        if !auth_config.rp_origins.is_empty() { return auth_config.rp_origins.clone() }
        let proto = if server_config.tls.is_some() { "https" } else { "http" };
        let port = server_config.sock_addr.port();
        let host = &auth_config.rp_id;
        let mut origins = vec![ format!("{proto}://{host}:{port}") ];
        if (proto == "https" && port == 443) || (proto == "http" && port == 80) { origins.push( format!("{proto}://{host}")); }
        origins
    }

    /* #region middleware ****************************************************************************************/

    async fn require_roles (State((auth, required)): State<(Arc<ServerAuth>,RoleSet)>, mut req: Request, next: Next)->Response {
        match auth.request_user( req.headers()).await {
            Some(user) if user.is_authorized( required) => {
                req.extensions_mut().insert( user); // handlers can use `Extension<Arc<ConnUser>>`
                next.run( req).await
            }
            Some(_) => (StatusCode::FORBIDDEN, "not authorized").into_response(),
            None => (StatusCode::UNAUTHORIZED, "not authenticated").into_response(),
        }
    }

    /// protect all routes of `router` (which should only contain the routes to protect) so that they require an
    /// authenticated user with any of the `required` roles (any authenticated user if `required` is empty)
    pub fn protect (router: Router, auth: Arc<ServerAuth>, required: RoleSet)->Router {
        router.route_layer( middleware::from_fn_with_state( (auth, required), require_roles))
    }

    /* #endregion middleware */

    /* #region routes ********************************************************************************************/

    /// called with the session id of a logged out session (e.g. to close its websocket connections)
    pub type LogoutHook = Arc<dyn Fn(Arc<str>) + Send + Sync>;

    #[derive(Clone)]
    struct RouteState {
        auth: Arc<ServerAuth>,
        on_logout: LogoutHook,
    }

    /// the auth API routes. These should be nested under a path that is relative to the application document so that
    /// clients can use relative URLs (e.g. `router.nest("/<app>/auth", auth_router(..))`)
    pub fn auth_router (auth: Arc<ServerAuth>, on_logout: LogoutHook)->Router {
        let state = RouteState{ auth, on_logout };
        Router::new()
            .route( "/login/password", post( login_password))
            .route( "/login/totp", post( login_totp))
            .route( "/login/passkey/start", post( passkey_login_start))
            .route( "/login/passkey/finish", post( passkey_login_finish))
            .route( "/enroll/info", post( enroll_info))
            .route( "/enroll/passkey/start", post( passkey_enroll_start))
            .route( "/enroll/passkey/finish", post( passkey_enroll_finish))
            .route( "/enroll/totp/start", post( totp_enroll_start))
            .route( "/enroll/totp/finish", post( totp_enroll_finish))
            .route( "/enroll/password", post( password_enroll))
            .route( "/logout", post( logout))
            .route( "/user", get( user_info))
            .with_state( state)
    }

    fn error_response (e: AuthError)->Response {
        let status = match &e {
            AuthError::InvalidCredentials | AuthError::NotAuthenticated => StatusCode::UNAUTHORIZED,
            AuthError::NotAuthorized | AuthError::InvalidToken => StatusCode::FORBIDDEN,
            AuthError::Locked(_) => StatusCode::TOO_MANY_REQUESTS,
            AuthError::InvalidInput(_) | AuthError::UnknownCeremony | AuthError::MethodNotEnabled(_) | AuthError::WebAuthn(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR
        };
        let msg = if e.is_client_error() { e.to_string() } else {
            eprintln!("authentication error: {e}");
            "internal server error".to_string()
        };
        (status, Json( json!({ "error": msg }))).into_response()
    }

    fn login_response (auth: &ServerAuth, login: Option<LoginResult>)->Response {
        match login {
            Some(login) => {
                let body = json!({ "ok": true, "user": login.user.name.as_ref() });
                ([(header::SET_COOKIE, auth.session_cookie( &login)), (header::CACHE_CONTROL, HeaderValue::from_static("no-store"))], Json( body)).into_response()
            }
            None => Json( json!({ "ok": true })).into_response()
        }
    }

    fn remote (addr: &SocketAddr)->String { addr.ip().to_string() }

    #[derive(Deserialize)]
    struct PasswordLogin { username: String, password: String }

    async fn login_password (State(s): State<RouteState>, ConnectInfo(addr): ConnectInfo<SocketAddr>, Json(req): Json<PasswordLogin>)->Response {
        match s.auth.authenticator.login_password( req.username.trim(), &req.password, Some( &remote( &addr))).await {
            Ok(login) => login_response( &s.auth, Some(login)),
            Err(e) => error_response(e)
        }
    }

    #[derive(Deserialize)]
    struct TotpLogin { username: String, code: String }

    async fn login_totp (State(s): State<RouteState>, ConnectInfo(addr): ConnectInfo<SocketAddr>, Json(req): Json<TotpLogin>)->Response {
        match s.auth.authenticator.login_totp( req.username.trim(), &req.code, Some( &remote( &addr))).await {
            Ok(login) => login_response( &s.auth, Some(login)),
            Err(e) => error_response(e)
        }
    }

    #[derive(Deserialize)]
    struct PasskeyLoginStart { username: String }

    async fn passkey_login_start (State(s): State<RouteState>, ConnectInfo(addr): ConnectInfo<SocketAddr>, Json(req): Json<PasskeyLoginStart>)->Response {
        match s.auth.authenticator.passkey_login_start( req.username.trim(), Some( &remote( &addr))).await {
            Ok((ceremony, options)) => Json( json!({ "ceremony": ceremony, "options": options })).into_response(),
            Err(e) => error_response(e)
        }
    }

    #[derive(Deserialize)]
    struct PasskeyLoginFinish { ceremony: String, credential: PublicKeyCredential }

    async fn passkey_login_finish (State(s): State<RouteState>, ConnectInfo(addr): ConnectInfo<SocketAddr>, Json(req): Json<PasskeyLoginFinish>)->Response {
        match s.auth.authenticator.passkey_login_finish( &req.ceremony, &req.credential, Some( &remote( &addr))).await {
            Ok(login) => login_response( &s.auth, Some(login)),
            Err(e) => error_response(e)
        }
    }

    async fn enroll_target (s: &RouteState, headers: &HeaderMap, token: Option<&str>)->AuthResult<EnrollTarget> {
        let user = s.auth.request_user( headers).await;
        s.auth.authenticator.enroll_target( token, user.as_deref()).await
    }

    #[derive(Deserialize)]
    struct EnrollInfo { token: Option<String> }

    async fn enroll_info (State(s): State<RouteState>, headers: HeaderMap, Json(req): Json<EnrollInfo>)->Response {
        match enroll_target( &s, &headers, req.token.as_deref()).await {
            Ok(target) => {
                let mut cfg = s.auth.client_config();
                cfg["user"] = json!( target.user_name());
                cfg["invite"] = json!( matches!( target, EnrollTarget::Invite{..}));
                Json( cfg).into_response()
            }
            Err(e) => error_response(e)
        }
    }

    #[derive(Deserialize)]
    struct PasskeyEnrollStart { token: Option<String>, label: Option<String> }

    async fn passkey_enroll_start (State(s): State<RouteState>, headers: HeaderMap, Json(req): Json<PasskeyEnrollStart>)->Response {
        let res = match enroll_target( &s, &headers, req.token.as_deref()).await {
            Ok(target) => s.auth.authenticator.passkey_enroll_start( target, req.label.filter( |l| !l.trim().is_empty())).await,
            Err(e) => Err(e)
        };
        match res {
            Ok((ceremony, options)) => Json( json!({ "ceremony": ceremony, "options": options })).into_response(),
            Err(e) => error_response(e)
        }
    }

    #[derive(Deserialize)]
    struct PasskeyEnrollFinish { ceremony: String, credential: RegisterPublicKeyCredential }

    async fn passkey_enroll_finish (State(s): State<RouteState>, ConnectInfo(addr): ConnectInfo<SocketAddr>, Json(req): Json<PasskeyEnrollFinish>)->Response {
        match s.auth.authenticator.passkey_enroll_finish( &req.ceremony, &req.credential, Some( &remote( &addr))).await {
            Ok(login) => login_response( &s.auth, login),
            Err(e) => error_response(e)
        }
    }

    async fn totp_enroll_start (State(s): State<RouteState>, headers: HeaderMap, Json(req): Json<EnrollInfo>)->Response {
        let res = match enroll_target( &s, &headers, req.token.as_deref()).await {
            Ok(target) => s.auth.authenticator.totp_enroll_start( target).await,
            Err(e) => Err(e)
        };
        match res {
            Ok(enr) => Json( json!({ "ceremony": enr.ceremony, "url": enr.url, "qrPng": enr.qr_png, "secret": enr.secret })).into_response(),
            Err(e) => error_response(e)
        }
    }

    #[derive(Deserialize)]
    struct TotpEnrollFinish { ceremony: String, code: String, label: Option<String> }

    async fn totp_enroll_finish (State(s): State<RouteState>, ConnectInfo(addr): ConnectInfo<SocketAddr>, Json(req): Json<TotpEnrollFinish>)->Response {
        let label = req.label.as_deref().filter( |l| !l.trim().is_empty());
        match s.auth.authenticator.totp_enroll_finish( &req.ceremony, &req.code, label, Some( &remote( &addr))).await {
            Ok(login) => login_response( &s.auth, login),
            Err(e) => error_response(e)
        }
    }

    #[derive(Deserialize)]
    struct PasswordEnroll { token: Option<String>, password: String }

    async fn password_enroll (State(s): State<RouteState>, headers: HeaderMap, ConnectInfo(addr): ConnectInfo<SocketAddr>, Json(req): Json<PasswordEnroll>)->Response {
        let res = match enroll_target( &s, &headers, req.token.as_deref()).await {
            Ok(target) => s.auth.authenticator.password_enroll( target, &req.password, Some( &remote( &addr))).await,
            Err(e) => Err(e)
        };
        match res {
            Ok(login) => login_response( &s.auth, login),
            Err(e) => error_response(e)
        }
    }

    async fn logout (State(s): State<RouteState>, headers: HeaderMap)->Response {
        if let Some(token) = s.auth.session_token( &headers) {
            match s.auth.authenticator.logout( token).await {
                Ok(Some(session)) => (s.on_logout)( session),
                Ok(None) => {}
                Err(e) => return error_response(e)
            }
        }
        ([(header::SET_COOKIE, s.auth.clear_cookie())], Json( json!({ "ok": true }))).into_response()
    }

    async fn user_info (State(s): State<RouteState>, headers: HeaderMap)->Response {
        match s.auth.request_user( &headers).await {
            Some(user) => {
                let roles = s.auth.role_map().names_of( user.roles);
                ([(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
                 Json( json!({ "name": user.name.as_ref(), "roles": roles, "expires": user.expires }))).into_response()
            }
            None => error_response( AuthError::NotAuthenticated)
        }
    }

    /* #endregion routes */
}

/// stub implementation if the crate is built without the `auth` feature. Servers with an `auth` config refuse to start
#[cfg(not(feature="auth"))]
mod stub {
    use std::sync::Arc;
    use axum::{Router, http::HeaderMap};
    use serde_json::Value;
    use odin_common::auth::*;
    use crate::{ServerConfig, errors::{OdinServerResult, op_failed}};

    pub enum ServerAuth {} // uninhabited - can't be instantiated

    impl ServerAuth {
        pub async fn new (_server_config: &ServerConfig, _auth_config: &AuthConfig)->OdinServerResult<Self> {
            Err( op_failed("server config has 'auth' but odin_server was built without the 'auth' feature"))
        }
        pub fn config (&self)->&AuthConfig { match *self {} }
        pub fn role_map (&self)->&RoleMap { match *self {} }
        pub async fn request_user (&self, _headers: &HeaderMap)->Option<Arc<ConnUser>> { match *self {} }
        pub fn client_config (&self)->Value { match *self {} }
        pub fn route_roles (&self, _key: &str, _default_roles: &[String])->OdinServerResult<RoleSet> { match *self {} }
        pub fn ws_msg_acl (&self, _service_defaults: &[(String,Vec<String>)])->OdinServerResult<MsgAcl> { match *self {} }
        pub fn service_acl (&self, _service_defaults: &[(String,Vec<String>)])->OdinServerResult<MsgAcl> { match *self {} }
        pub async fn refresh (&self) { match *self {} }
        pub fn revalidate (&self, _user: &ConnUser)->Revalidation { match *self {} }
    }

    #[derive(Debug,Clone,Copy,PartialEq)]
    pub enum Revalidation { Valid, RolesChanged(RoleSet), Invalid }

    pub type LogoutHook = Arc<dyn Fn(Arc<str>) + Send + Sync>;

    pub fn protect (router: Router, auth: Arc<ServerAuth>, _required: RoleSet)->Router { match *auth {} }
    pub fn auth_router (auth: Arc<ServerAuth>, _on_logout: LogoutHook)->Router { match *auth {} }
}
