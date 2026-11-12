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

//! server type independent user authentication and role based authorization.
//!
//! This module root contains the types that are always compiled since they are referenced by general server
//! infrastructure such as [`crate::ws::WsConnection`]: role sets ([`RoleSet`], [`RoleMap`]), authenticated connection
//! users ([`ConnUser`]), message access control lists ([`MsgAcl`]) and the [`AuthConfig`].
//!
//! The actual authentication (user store, sessions, passkey/TOTP/password methods) is in the [`Authenticator`] and
//! [`AuthStore`] types, which require the `auth` feature (adding SQLite, WebAuthn and TOTP dependencies).
//!
//! Roles are configured as a list of up to [`MAX_ROLES`] names, which are mapped to bit positions of a `u64`
//! ([`RoleSet`]), i.e. authorization checks are a single bitwise AND. A resource requires a [`RoleSet`] and a user
//! is authorized if the user has *any* of the required roles. An empty required set means any authenticated user.

use std::{collections::HashMap, fmt, sync::Arc, time::Duration};
use serde::{Serialize, Deserialize};
use thiserror::Error;
use crate::{datetime::{deserialize_duration, serialize_duration}, fs::EnvPathBuf};

#[cfg(feature="auth")] mod store;
#[cfg(feature="auth")] pub use store::*;

#[cfg(feature="auth")] mod authenticator;
#[cfg(feature="auth")] pub use authenticator::*;

/// max number of configurable roles (bits in a [`RoleSet`])
pub const MAX_ROLES: usize = 64;

/* #region errors ****************************************************************************************************/

#[derive(Error,Debug,Clone,PartialEq)]
pub enum AuthError {
    #[error("invalid credentials")]
    InvalidCredentials,

    #[error("too many failed attempts, retry in {0} sec")]
    Locked(u64),

    #[error("not authenticated")]
    NotAuthenticated,

    #[error("not authorized")]
    NotAuthorized,

    #[error("invalid or expired enrollment token")]
    InvalidToken,

    #[error("unknown or expired authentication request")]
    UnknownCeremony,

    #[error("authentication method not enabled: {0}")]
    MethodNotEnabled(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("auth config error: {0}")]
    Config(String),

    #[error("auth database error: {0}")]
    Db(String),

    #[error("webauthn error: {0}")]
    WebAuthn(String),

    #[error("internal auth error: {0}")]
    Internal(String),
}

impl AuthError {
    /// is this an error that should be reported to the client as an authentication failure (as opposed to an
    /// internal server error)
    pub fn is_client_error (&self)->bool {
        !matches!( self, AuthError::Config(_) | AuthError::Db(_) | AuthError::Internal(_))
    }
}

pub type AuthResult<T> = Result<T,AuthError>;

/* #endregion errors */

/* #region roles *****************************************************************************************************/

/// a set of (up to 64) roles, represented as bits of a `u64`. Bit positions are defined by a [`RoleMap`]
#[derive(Clone,Copy,PartialEq,Eq,Hash,Default)]
pub struct RoleSet(u64);

impl RoleSet {
    pub const NONE: RoleSet = RoleSet(0);
    pub const ALL: RoleSet = RoleSet(u64::MAX);

    pub const fn from_bits (bits: u64)->Self { RoleSet(bits) }
    pub const fn bits (&self)->u64 { self.0 }
    pub const fn is_empty (&self)->bool { self.0 == 0 }

    /// do `self` (user roles) satisfy `required` (resource roles)? This is the case if `required` is empty (any
    /// authenticated user) or if the user has any of the required roles
    #[inline]
    pub const fn satisfies (&self, required: RoleSet)->bool {
        required.0 == 0 || (self.0 & required.0) != 0
    }

    pub const fn contains (&self, idx: usize)->bool { idx < MAX_ROLES && (self.0 & (1u64 << idx)) != 0 }
    pub const fn with (self, idx: usize)->Self { if idx < MAX_ROLES { RoleSet( self.0 | (1u64 << idx)) } else { self } }
    pub const fn union (self, other: RoleSet)->Self { RoleSet( self.0 | other.0) }
}

impl fmt::Debug for RoleSet {
    fn fmt (&self, f: &mut fmt::Formatter<'_>)->fmt::Result { write!( f, "RoleSet({:#x})", self.0) }
}

/// mapping between configured role names and [`RoleSet`] bit positions
#[derive(Debug,Clone,Default)]
pub struct RoleMap {
    names: Vec<String>,
    index: HashMap<String,usize>,
}

impl RoleMap {
    pub fn new<S: AsRef<str>> (names: &[S])->AuthResult<Self> {
        if names.len() > MAX_ROLES { return Err( AuthError::Config( format!("too many roles ({} > {MAX_ROLES})", names.len()))) }
        let mut index = HashMap::with_capacity( names.len());
        let mut v = Vec::with_capacity( names.len());
        for (i, name) in names.iter().enumerate() {
            let name = name.as_ref().trim();
            if !is_valid_role_name( name) { return Err( AuthError::Config( format!("invalid role name '{name}'"))) }
            if index.insert( name.to_string(), i).is_some() { return Err( AuthError::Config( format!("duplicated role '{name}'"))) }
            v.push( name.to_string());
        }
        Ok( RoleMap{ names: v, index })
    }

    pub fn len (&self)->usize { self.names.len() }
    pub fn is_empty (&self)->bool { self.names.is_empty() }
    pub fn names (&self)->&[String] { &self.names }
    pub fn contains (&self, name: &str)->bool { self.index.contains_key( name) }

    /// role set for given names. Unknown role names are an error (use for configured resource roles)
    pub fn role_set<S: AsRef<str>> (&self, names: &[S])->AuthResult<RoleSet> {
        let mut rs = RoleSet::NONE;
        for name in names {
            let name = name.as_ref().trim();
            match self.index.get( name) {
                Some(idx) => rs = rs.with( *idx),
                None => return Err( AuthError::Config( format!("unknown role '{name}'")))
            }
        }
        Ok(rs)
    }

    /// role set for a comma separated list of role names. Unknown names are ignored (use for stored user roles,
    /// which might refer to roles that have been removed from the config)
    pub fn parse_lenient (&self, csv: &str)->RoleSet {
        csv.split(',').map( |s| s.trim()).filter( |s| !s.is_empty())
            .filter_map( |s| self.index.get(s)).fold( RoleSet::NONE, |rs, idx| rs.with( *idx))
    }

    /// names of roles in the given set (in config order)
    pub fn names_of (&self, rs: RoleSet)->Vec<&str> {
        self.names.iter().enumerate().filter( |(i,_)| rs.contains(*i)).map( |(_,n)| n.as_str()).collect()
    }
}

pub fn is_valid_role_name (name: &str)->bool {
    !name.is_empty() && name.len() <= 32 && name.chars().all( |c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// valid user names are 1-64 chars of ASCII alphanumeric, '.', '_', '-' or '@' (this is also safe for TOTP
/// account names and cookie/URL contexts)
pub fn is_valid_user_name (name: &str)->bool {
    !name.is_empty() && name.len() <= 64 && name.chars().all( |c| c.is_ascii_alphanumeric() || matches!( c, '.' | '_' | '-' | '@'))
}

/* #endregion roles */

/* #region connection user *******************************************************************************************/

/// an authenticated user of a connection or request. Connections of servers without authentication use `None`
/// in `Option<Arc<ConnUser>>` contexts (see [`is_authorized`])
#[derive(Debug,Clone)]
pub struct ConnUser {
    pub user_id: i64,
    pub name: Arc<str>,
    pub roles: RoleSet,
    pub session: Arc<str>, // session id (hash of the session token, never the token itself)
    pub expires: i64,      // session expiration (epoch secs)
}

impl ConnUser {
    #[inline]
    pub fn is_authorized (&self, required: RoleSet)->bool { self.roles.satisfies( required) }
}

/// authorization check for optional users - `None` means the server does not use authentication, i.e. all
/// resources are accessible
#[inline]
pub fn is_authorized (user: Option<&ConnUser>, required: RoleSet)->bool {
    match user {
        Some(user) => user.roles.satisfies( required),
        None => true
    }
}

/* #endregion connection user */

/* #region message ACL ***********************************************************************************************/

/// role requirements for messages identified by a module path and a message type (e.g. incoming websocket
/// messages). Keys are specified as `"<mod_path>:<msg_type>"` or `"<mod_path>"` / `"<mod_path>:*"` for all message
/// types of a module. Lookup does not allocate.
#[derive(Debug,Clone,Default)]
pub struct MsgAcl {
    map: HashMap<String,HashMap<String,RoleSet>>,
}

impl MsgAcl {
    pub fn new ()->Self { MsgAcl::default() }

    pub fn is_empty (&self)->bool { self.map.is_empty() }

    /// set (replace) the roles for `key` (`"<mod_path>[:<msg_type>|*]"`)
    pub fn set (&mut self, key: &str, roles: RoleSet) {
        let (mod_path, msg_type) = match key.split_once(':') {
            Some((m,t)) => (m.trim(), t.trim()),
            None => (key.trim(), "*")
        };
        self.map.entry( mod_path.to_string()).or_default().insert( msg_type.to_string(), roles);
    }

    /// required roles for a message, `None` if there are no requirements (i.e. the message is not protected)
    #[inline]
    pub fn required (&self, mod_path: &str, msg_type: &str)->Option<RoleSet> {
        self.map.get( mod_path).and_then( |m| m.get( msg_type).or_else( || m.get("*")).copied())
    }

    /// add role requirements from a key->role-names map (e.g. from a config), resolving names with `role_map`
    pub fn set_from_names (&mut self, map: &HashMap<String,Vec<String>>, role_map: &RoleMap)->AuthResult<()> {
        for (key, names) in map {
            self.set( key, role_map.role_set( names)?);
        }
        Ok(())
    }
}

/* #endregion message ACL */

/* #region config ****************************************************************************************************/

/// the supported authentication methods
#[derive(Debug,Clone,Copy,PartialEq,Eq,Hash,Serialize,Deserialize)]
pub enum AuthMethod {
    /// WebAuthn passkeys (platform authenticators or security keys). This requires a secure context, i.e. either
    /// `localhost` or HTTPS with a DNS name (no IP addresses)
    Passkey,
    /// time based one time passwords (RFC 6238) from authenticator apps. Works on IP-only LANs
    Totp,
    /// username/password (stored as argon2 hashes)
    Password,
}

impl AuthMethod {
    pub fn as_str (&self)->&'static str {
        match self { AuthMethod::Passkey => "passkey", AuthMethod::Totp => "totp", AuthMethod::Password => "password" }
    }
}

/// authentication/authorization configuration. This is normally an optional part of a server config (e.g. the
/// `auth` field of `odin_server::ServerConfig`) - if it is not set the server does not authenticate users
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct AuthConfig {
    /// enabled authentication methods (in the order they are offered to the user)
    #[serde(default="default_methods")]
    pub methods: Vec<AuthMethod>,

    /// the configured role names (max 64)
    pub roles: Vec<String>,

    /// WebAuthn relying party id (DNS name of the server, or "localhost"). Passkeys do not work with IP addresses
    #[serde(default="default_rp_id")]
    pub rp_id: String,

    /// allowed WebAuthn origins (e.g. `"https://odin.example.org:9009"`). If empty this is computed by the server
    /// from `rp_id` and the server address
    #[serde(default)]
    pub rp_origins: Vec<String>,

    /// name shown by authenticators (passkey dialogs, TOTP apps)
    #[serde(default="default_rp_name")]
    pub rp_name: String,

    /// max session duration
    #[serde(default="default_session_ttl", deserialize_with="deserialize_duration", serialize_with="serialize_duration")]
    pub session_ttl: Duration,

    /// validity of enrollment (invite) tokens
    #[serde(default="default_invite_ttl", deserialize_with="deserialize_duration", serialize_with="serialize_duration")]
    pub invite_ttl: Duration,

    /// user database (default `$ODIN_ROOT/data/odin_server/auth.db`)
    #[serde(default)]
    pub db_path: Option<EnvPathBuf>,

    /// role requirements for explicitly protected server routes (key -> role names). This overrides defaults of
    /// the code that adds the protected route
    #[serde(default)]
    pub route_roles: HashMap<String,Vec<String>>,

    /// role requirements for incoming websocket messages ("<mod_path>:<msg_type>" -> role names). This overrides
    /// defaults of services
    #[serde(default)]
    pub ws_msg_roles: HashMap<String,Vec<String>>,

    /// require an authenticated session for proxied URLs (otherwise the server can be used as an open proxy)
    #[serde(default="default_true")]
    pub protect_proxies: bool,

    /// allow authentication over plain HTTP on non-loopback addresses. Credentials and session cookies are then
    /// transmitted unencrypted - only use in isolated networks
    #[serde(default)]
    pub allow_insecure: bool,

    /// number of failed login attempts (per user name and per client address) before login is temporarily locked
    #[serde(default="default_max_failures")]
    pub max_failures: u32,

    /// initial lockout duration (doubled for each additional failure, max 1h)
    #[serde(default="default_lockout", deserialize_with="deserialize_duration", serialize_with="serialize_duration")]
    pub lockout: Duration,

    /// minimum password length (if `Password` method is enabled)
    #[serde(default="default_min_password_len")]
    pub min_password_len: usize,
}

fn default_methods ()->Vec<AuthMethod> { vec![ AuthMethod::Passkey, AuthMethod::Totp ] }
fn default_rp_id ()->String { "localhost".to_string() }
fn default_rp_name ()->String { "ODIN".to_string() }
fn default_session_ttl ()->Duration { Duration::from_secs( 12 * 3600) }
fn default_invite_ttl ()->Duration { Duration::from_secs( 24 * 3600) }
fn default_true ()->bool { true }
fn default_max_failures ()->u32 { 5 }
fn default_lockout ()->Duration { Duration::from_secs( 30) }
fn default_min_password_len ()->usize { 12 }

impl AuthConfig {
    /// a config with default values for the given roles
    pub fn with_roles<S: ToString> (roles: &[S])->Self {
        AuthConfig {
            methods: default_methods(),
            roles: roles.iter().map( |r| r.to_string()).collect(),
            rp_id: default_rp_id(),
            rp_origins: Vec::new(),
            rp_name: default_rp_name(),
            session_ttl: default_session_ttl(),
            invite_ttl: default_invite_ttl(),
            db_path: None,
            route_roles: HashMap::new(),
            ws_msg_roles: HashMap::new(),
            protect_proxies: true,
            allow_insecure: false,
            max_failures: default_max_failures(),
            lockout: default_lockout(),
            min_password_len: default_min_password_len(),
        }
    }

    pub fn is_enabled (&self, method: AuthMethod)->bool { self.methods.contains( &method) }

    /// the role map for the configured roles
    pub fn role_map (&self)->AuthResult<RoleMap> { RoleMap::new( &self.roles) }

    /// check config consistency (roles, methods and role names used in route/message ACLs)
    pub fn validate (&self)->AuthResult<RoleMap> {
        let role_map = self.role_map()?;
        if self.methods.is_empty() { return Err( AuthError::Config("no authentication methods".into())) }
        for names in self.route_roles.values().chain( self.ws_msg_roles.values()) {
            role_map.role_set( names)?;
        }
        if self.rp_id.parse::<std::net::IpAddr>().is_ok() && self.is_enabled( AuthMethod::Passkey) {
            return Err( AuthError::Config( format!("passkeys require a DNS name or localhost as rp_id, not an IP address ({})", self.rp_id)))
        }
        Ok( role_map)
    }
}

/* #endregion config */
