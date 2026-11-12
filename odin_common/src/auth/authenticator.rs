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

//! the server type independent [`Authenticator`], which implements sessions, login (passkey, TOTP, password),
//! enrollment of credentials and user administration on top of an [`AuthStore`].
//!
//! Sessions are identified by random 256 bit tokens (normally stored in a cookie by the server). Only the SHA-256
//! hash of the token is stored (and used as session id in [`ConnUser`]). Session and user lookups are cached in
//! memory. The user cache is reloaded from the store every [`USER_REFRESH`] so that changes by admin tools (e.g.
//! revoked or disabled users, changed roles) take effect in running servers.

use std::{collections::HashMap, path::PathBuf, sync::{Arc, LazyLock, Mutex}, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Serialize, Deserialize};
use sha2::{Digest, Sha256};
use totp_rs::{Algorithm, Builder as TotpBuilder, Secret, Totp};
use webauthn_rs::prelude::{Passkey, PasskeyAuthentication, PasskeyRegistration, Url, Uuid, Webauthn, WebauthnBuilder};

// re-export the WebAuthn protocol types that servers have to (de)serialize
pub use webauthn_rs::prelude::{CreationChallengeResponse, RequestChallengeResponse, RegisterPublicKeyCredential, PublicKeyCredential};

use super::*;

/// how often the user cache is reloaded from the store
pub const USER_REFRESH: Duration = Duration::from_secs(30);
/// max duration of a pending passkey/TOTP ceremony (between start and finish requests)
pub const CEREMONY_TTL: Duration = Duration::from_secs(300);
const MAX_CEREMONIES: usize = 1000;
const MAX_TOTP_ENROLL_ATTEMPTS: u32 = 5;
const MAX_LOCKOUT: Duration = Duration::from_secs(3600);
const TOKEN_BYTES: usize = 32;

static DUMMY_HASH: LazyLock<String> = LazyLock::new( || password_auth::generate_hash("no such user - dummy password"));

pub fn now_secs ()->i64 {
    SystemTime::now().duration_since( UNIX_EPOCH).map( |d| d.as_secs() as i64).unwrap_or(0)
}

/// random URL-safe token with `n` bytes of entropy
pub fn random_token (n: usize)->AuthResult<String> {
    let mut buf = vec![0u8; n];
    getrandom::fill( &mut buf).map_err( |e| AuthError::Internal( format!("no random source: {e}")))?;
    Ok( URL_SAFE_NO_PAD.encode( &buf))
}

/// the (session/invite) token hash that is stored and used as id
pub fn hash_token (token: &str)->String {
    URL_SAFE_NO_PAD.encode( Sha256::digest( token.as_bytes()))
}

fn random_uuid ()->AuthResult<Uuid> {
    let mut b = [0u8; 16];
    getrandom::fill( &mut b).map_err( |e| AuthError::Internal( format!("no random source: {e}")))?;
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // RFC 4122 variant
    Ok( Uuid::from_bytes(b))
}

/// default user database location: `$ODIN_ROOT/data/auth/auth.db`
pub fn default_db_path ()->PathBuf {
    odin_build::data_dir().join("auth").join("auth.db")
}

/* #region public result types ***************************************************************************************/

/// result of a successful login (or enrollment with an invite token)
#[derive(Debug,Clone)]
pub struct LoginResult {
    pub token: String,       // the session token to store in the client (e.g. as cookie value)
    pub user: Arc<ConnUser>,
    pub expires: i64,        // epoch secs
}

/// what is required to enroll a TOTP authenticator app
#[derive(Debug,Clone,Serialize)]
pub struct TotpEnrollment {
    pub ceremony: String,
    pub url: String,       // otpauth:// URL
    pub qr_png: String,    // base64 encoded PNG of the otpauth URL QR code
    pub secret: String,    // base32 encoded secret (for manual entry)
}

/// what we enroll credentials for
#[derive(Debug,Clone)]
pub enum EnrollTarget {
    /// first credential(s) of a user, authorized by an admin created invite token
    Invite { token_hash: String, user_id: i64, user_name: String },
    /// additional credential of a logged in user
    User { user_id: i64, user_name: String },
}

impl EnrollTarget {
    pub fn user_name (&self)->&str {
        match self { EnrollTarget::Invite{user_name,..} | EnrollTarget::User{user_name,..} => user_name.as_str() }
    }
    fn user_id (&self)->i64 {
        match self { EnrollTarget::Invite{user_id,..} | EnrollTarget::User{user_id,..} => *user_id }
    }
}

/// session check result for connections that are already established (see [`Authenticator::revalidate`])
#[derive(Debug,Clone,Copy,PartialEq)]
pub enum Revalidation {
    Valid,
    RolesChanged(RoleSet),
    Invalid,
}

/* #endregion public result types */

/* #region internal state ********************************************************************************************/

#[derive(Debug,Clone)]
struct CachedUser {
    name: Arc<str>,
    roles: RoleSet,
    epoch: i64,
    disabled: bool,
    expires: Option<i64>,
}

impl CachedUser {
    fn is_active (&self, now: i64)->bool { !self.disabled && self.expires.map( |e| e > now).unwrap_or(true) }
}

#[derive(Debug,Clone)]
struct CachedSession {
    user_id: i64,
    epoch: i64,
    expires: i64,
}

enum CeremonyKind {
    PasskeyAuth { user_id: i64, user_name: String, state: PasskeyAuthentication },
    PasskeyReg { target: EnrollTarget, state: PasskeyRegistration, label: Option<String> },
    TotpReg { target: EnrollTarget, secret: Vec<u8>, attempts: u32 },
}

struct Ceremony {
    kind: CeremonyKind,
    expires: Instant,
}

#[derive(Debug)]
struct Failure {
    count: u32,
    locked_until: Option<Instant>,
    last: Instant,
}

#[derive(Default)]
struct AuthState {
    users: HashMap<i64,CachedUser>,
    users_loaded: Option<Instant>,
    sessions: HashMap<String,CachedSession>,
    ceremonies: HashMap<String,Ceremony>,
    failures: HashMap<String,Failure>,
}

#[derive(Serialize,Deserialize)]
struct TotpData {
    secret: String,  // base32
    last_step: u64,  // last accepted time step (codes can only be used once)
}

/* #endregion internal state */

/// the authenticator for a server (or admin tool). Can be shared between tasks (e.g. as `Arc<Authenticator>`)
pub struct Authenticator {
    config: AuthConfig,
    role_map: RoleMap,
    store: AuthStore,
    webauthn: Option<Webauthn>,
    state: Mutex<AuthState>,
}

impl Authenticator {

    /// create an authenticator with the database configured in `config` (or the [`default_db_path`]).
    /// `server_origins` are the URL origins of the server (e.g. "http://localhost:9009"), which are used as allowed
    /// WebAuthn origins if `config.rp_origins` is empty. If there are no origins (e.g. for admin tools) passkey
    /// ceremonies are not available
    pub async fn new (config: AuthConfig, server_origins: &[String])->AuthResult<Self> {
        let db_path = config.db_path.as_ref().map( |p| p.0.clone()).unwrap_or_else( default_db_path);
        let store = AuthStore::open( &db_path).await?;
        Self::with_store( config, store, server_origins)
    }

    pub fn with_store (config: AuthConfig, store: AuthStore, server_origins: &[String])->AuthResult<Self> {
        let role_map = config.validate()?;
        let webauthn = if config.is_enabled( AuthMethod::Passkey) {
            let origins: Vec<&String> = if config.rp_origins.is_empty() { server_origins.iter().collect() } else { config.rp_origins.iter().collect() };
            if origins.is_empty() { None } else { Some( create_webauthn( &config, &origins)?) }
        } else { None };

        Ok( Authenticator{ config, role_map, store, webauthn, state: Mutex::new( AuthState::default()) })
    }

    pub fn config (&self)->&AuthConfig { &self.config }
    pub fn role_map (&self)->&RoleMap { &self.role_map }
    pub fn store (&self)->&AuthStore { &self.store }

    /// the methods that can be offered to clients (passkeys only if WebAuthn is configured)
    pub fn available_methods (&self)->Vec<AuthMethod> {
        self.config.methods.iter().copied().filter( |m| *m != AuthMethod::Passkey || self.webauthn.is_some()).collect()
    }

    pub fn is_available (&self, method: AuthMethod)->bool {
        match method {
            AuthMethod::Passkey => self.webauthn.is_some(),
            _ => self.config.is_enabled( method)
        }
    }

    fn check_available (&self, method: AuthMethod)->AuthResult<()> {
        if self.is_available( method) { Ok(()) } else { Err( AuthError::MethodNotEnabled( method.as_str().to_string())) }
    }

    fn webauthn (&self)->AuthResult<&Webauthn> {
        self.webauthn.as_ref().ok_or_else( || AuthError::MethodNotEnabled("passkey".to_string()))
    }

    /* #region sessions ******************************************************************************************/

    /// the user of a valid session token, None if the token is unknown, expired or revoked
    pub async fn session_user (&self, token: &str)->Option<Arc<ConnUser>> {
        if token.is_empty() || token.len() > 128 { return None }
        let hash = hash_token( token);
        self.ensure_fresh_users().await;

        let cached = self.state.lock().unwrap().sessions.get( &hash).cloned();
        let session = match cached {
            Some(s) => s,
            None => {
                let rec = self.store.session( &hash).await.ok().flatten()?;
                if self.cached_user( rec.user_id).is_none() { // user might have been added after the last refresh
                    if let Ok(Some(u)) = self.store.user( rec.user_id).await { self.cache_user( &u); }
                }
                let s = CachedSession{ user_id: rec.user_id, epoch: rec.epoch, expires: rec.expires };
                self.state.lock().unwrap().sessions.insert( hash.clone(), s.clone());
                s
            }
        };
        self.conn_user( &hash, &session)
    }

    fn conn_user (&self, hash: &str, session: &CachedSession)->Option<Arc<ConnUser>> {
        let now = now_secs();
        let state = self.state.lock().unwrap();
        let user = state.users.get( &session.user_id)?;
        if session.expires <= now || user.epoch != session.epoch || !user.is_active( now) { return None }
        Some( Arc::new( ConnUser{ user_id: session.user_id, name: user.name.clone(), roles: user.roles, session: Arc::from( hash), expires: session.expires }))
    }

    /// check if the session of an established connection is still valid and if its roles have changed. This only
    /// uses cached data and hence should be called after [`Self::refresh`]
    pub fn revalidate (&self, user: &ConnUser)->Revalidation {
        let now = now_secs();
        let state = self.state.lock().unwrap();
        let Some(session) = state.sessions.get( user.session.as_ref()) else { return Revalidation::Invalid };
        let Some(cu) = state.users.get( &session.user_id) else { return Revalidation::Invalid };
        if session.expires <= now || cu.epoch != session.epoch || !cu.is_active( now) {
            Revalidation::Invalid
        } else if cu.roles != user.roles {
            Revalidation::RolesChanged( cu.roles)
        } else {
            Revalidation::Valid
        }
    }

    /// end the session for the given token. Returns the session id (token hash) if there was such a session
    pub async fn logout (&self, token: &str)->AuthResult<Option<Arc<str>>> {
        let hash = hash_token( token);
        self.state.lock().unwrap().sessions.remove( &hash);
        if self.store.remove_session( &hash).await? { Ok( Some( Arc::from( hash.as_str()))) } else { Ok(None) }
    }

    /// reload the user cache from the store and purge expired sessions, invites, ceremonies and lockouts
    pub async fn refresh (&self)->AuthResult<()> {
        let now = now_secs();
        self.store.purge_expired( now).await?;
        self.reload_users().await?;

        let t = Instant::now();
        let mut state = self.state.lock().unwrap();
        state.sessions.retain( |_,s| s.expires > now);
        state.ceremonies.retain( |_,c| c.expires > t);
        state.failures.retain( |_,f| f.locked_until.map( |l| l > t).unwrap_or(false) || t.duration_since( f.last) < MAX_LOCKOUT);
        Ok(())
    }

    async fn reload_users (&self)->AuthResult<()> {
        let users = self.store.users().await?;
        let map: HashMap<i64,CachedUser> = users.iter().map( |u| (u.id, self.to_cached_user( u))).collect();
        let mut state = self.state.lock().unwrap();
        state.users = map;
        state.users_loaded = Some( Instant::now());
        Ok(())
    }

    async fn ensure_fresh_users (&self) {
        let stale = self.state.lock().unwrap().users_loaded.map( |t| t.elapsed() > USER_REFRESH).unwrap_or(true);
        if stale {
            if let Err(e) = self.reload_users().await { eprintln!("failed to reload auth users: {e}"); }
        }
    }

    fn to_cached_user (&self, u: &UserRecord)->CachedUser {
        CachedUser{ name: Arc::from( u.name.as_str()), roles: self.role_map.parse_lenient( &u.roles), epoch: u.epoch, disabled: u.disabled, expires: u.expires }
    }

    fn cache_user (&self, u: &UserRecord) {
        let cu = self.to_cached_user( u);
        self.state.lock().unwrap().users.insert( u.id, cu);
    }

    fn cached_user (&self, user_id: i64)->Option<CachedUser> {
        self.state.lock().unwrap().users.get( &user_id).cloned()
    }

    async fn create_session (&self, user: &UserRecord, remote: Option<&str>)->AuthResult<LoginResult> {
        let token = random_token( TOKEN_BYTES)?;
        let hash = hash_token( &token);
        let now = now_secs();
        let expires = now + self.config.session_ttl.as_secs() as i64;
        let rec = SessionRecord{ token_hash: hash.clone(), user_id: user.id, epoch: user.epoch, created: now, expires, remote: remote.map( |s| s.to_string()) };
        self.store.add_session( &rec).await?;

        self.cache_user( user);
        let session = CachedSession{ user_id: user.id, epoch: user.epoch, expires };
        self.state.lock().unwrap().sessions.insert( hash.clone(), session.clone());
        let conn_user = self.conn_user( &hash, &session).ok_or( AuthError::InvalidCredentials)?;
        Ok( LoginResult{ token, user: conn_user, expires })
    }

    /* #endregion sessions */

    /* #region rate limiting *************************************************************************************/

    fn limit_keys (user_name: &str, remote: Option<&str>)->Vec<String> {
        let mut keys = vec![ format!("u:{user_name}") ];
        if let Some(r) = remote { keys.push( format!("a:{r}")); }
        keys
    }

    fn check_locked (&self, keys: &[String])->AuthResult<()> {
        let now = Instant::now();
        let state = self.state.lock().unwrap();
        for k in keys {
            if let Some(until) = state.failures.get(k).and_then( |f| f.locked_until) {
                if until > now { return Err( AuthError::Locked( (until - now).as_secs().max(1))) }
            }
        }
        Ok(())
    }

    fn record_failure (&self, keys: &[String]) {
        let now = Instant::now();
        let max = self.config.max_failures.max(1);
        let mut state = self.state.lock().unwrap();
        for k in keys {
            let f = state.failures.entry( k.clone()).or_insert( Failure{ count: 0, locked_until: None, last: now });
            f.count += 1;
            f.last = now;
            if f.count >= max {
                let factor = 1u32 << (f.count - max).min(16);
                f.locked_until = Some( now + (self.config.lockout * factor).min( MAX_LOCKOUT));
            }
        }
    }

    fn clear_failures (&self, keys: &[String]) {
        let mut state = self.state.lock().unwrap();
        for k in keys { state.failures.remove( k); }
    }

    /// common post-verification processing of login attempts
    async fn conclude_login (&self, user: Option<&UserRecord>, verified: bool, keys: &[String], remote: Option<&str>)->AuthResult<LoginResult> {
        match user {
            Some(user) if verified && user.is_active( now_secs()) => {
                self.clear_failures( keys);
                self.create_session( user, remote).await
            }
            _ => {
                self.record_failure( keys);
                Err( AuthError::InvalidCredentials)
            }
        }
    }

    /* #endregion rate limiting */

    /* #region ceremonies ****************************************************************************************/

    fn add_ceremony (&self, kind: CeremonyKind)->AuthResult<String> {
        let id = random_token( 16)?;
        let now = Instant::now();
        let mut state = self.state.lock().unwrap();
        state.ceremonies.retain( |_,c| c.expires > now);
        if state.ceremonies.len() >= MAX_CEREMONIES { return Err( AuthError::Internal("too many pending authentication requests".into())) }
        state.ceremonies.insert( id.clone(), Ceremony{ kind, expires: now + CEREMONY_TTL });
        Ok(id)
    }

    fn take_ceremony (&self, id: &str)->AuthResult<CeremonyKind> {
        let mut state = self.state.lock().unwrap();
        match state.ceremonies.remove( id) {
            Some(c) if c.expires > Instant::now() => Ok( c.kind),
            _ => Err( AuthError::UnknownCeremony)
        }
    }

    /* #endregion ceremonies */

    /* #region login *********************************************************************************************/

    pub async fn login_password (&self, user_name: &str, password: &str, remote: Option<&str>)->AuthResult<LoginResult> {
        self.check_available( AuthMethod::Password)?;
        let keys = Self::limit_keys( user_name, remote);
        self.check_locked( &keys)?;

        let user = self.store.user_by_name( user_name).await?;
        let hash = match &user {
            Some(u) => self.store.credentials( u.id, Some(CredentialKind::Password)).await?.into_iter().next().map( |c| c.data),
            None => None
        };
        let has_hash = hash.is_some();
        let hash = hash.unwrap_or_else( || DUMMY_HASH.clone()); // verify anyways so that timing does not reveal unknown users
        let password = password.to_string();
        let verified = tokio::task::spawn_blocking( move || password_auth::verify_password( password, &hash).is_ok()).await
            .map_err( |e| AuthError::Internal( e.to_string()))?;

        self.conclude_login( user.as_ref(), verified && has_hash, &keys, remote).await
    }

    pub async fn login_totp (&self, user_name: &str, code: &str, remote: Option<&str>)->AuthResult<LoginResult> {
        self.check_available( AuthMethod::Totp)?;
        let keys = Self::limit_keys( user_name, remote);
        self.check_locked( &keys)?;

        let user = self.store.user_by_name( user_name).await?;
        let mut verified = false;
        if let Some(u) = &user {
            let now = now_secs() as u64;
            for cred in self.store.credentials( u.id, Some(CredentialKind::Totp)).await? {
                let Ok(mut data) = serde_json::from_str::<TotpData>( &cred.data) else { continue };
                let Ok(secret) = Secret::try_from_base32( &data.secret) else { continue };
                let totp = build_totp( secret.as_bytes().to_vec(), &self.config.rp_name, &u.name)?;
                if let Some(step) = totp.check( code.trim(), now) {
                    if step > data.last_step { // replay protection
                        data.last_step = step;
                        let json = serde_json::to_string( &data).map_err( |e| AuthError::Internal( e.to_string()))?;
                        self.store.update_credential( cred.id, &json, now as i64).await?;
                        verified = true;
                        break;
                    }
                }
            }
        }
        self.conclude_login( user.as_ref(), verified, &keys, remote).await
    }

    /// start passkey login for `user_name`. Returns the ceremony id and the options for `navigator.credentials.get()`
    pub async fn passkey_login_start (&self, user_name: &str, remote: Option<&str>)->AuthResult<(String,RequestChallengeResponse)> {
        let webauthn = self.webauthn()?;
        let keys = Self::limit_keys( user_name, remote);
        self.check_locked( &keys)?;

        let user = match self.store.user_by_name( user_name).await? {
            Some(u) if u.is_active( now_secs()) => u,
            _ => { self.record_failure( &keys); return Err( AuthError::InvalidCredentials) }
        };
        let passkeys = self.passkeys( user.id).await?;
        if passkeys.is_empty() { self.record_failure( &keys); return Err( AuthError::InvalidCredentials) }

        let (rcr, state) = webauthn.start_passkey_authentication( &passkeys.iter().map( |(_,pk)| pk.clone()).collect::<Vec<_>>())
            .map_err( |e| AuthError::WebAuthn( e.to_string()))?;
        let id = self.add_ceremony( CeremonyKind::PasskeyAuth{ user_id: user.id, user_name: user.name.clone(), state })?;
        Ok( (id, rcr) )
    }

    pub async fn passkey_login_finish (&self, ceremony: &str, credential: &PublicKeyCredential, remote: Option<&str>)->AuthResult<LoginResult> {
        let webauthn = self.webauthn()?;
        let CeremonyKind::PasskeyAuth{ user_id, user_name, state } = self.take_ceremony( ceremony)? else { return Err( AuthError::UnknownCeremony) };
        let keys = Self::limit_keys( &user_name, remote);
        self.check_locked( &keys)?;

        let user = self.store.user( user_id).await?;
        let verified = match webauthn.finish_passkey_authentication( credential, &state) {
            Ok(res) => {
                // update the stored credential (signature counter, backup state)
                for (rec, mut pk) in self.passkeys( user_id).await? {
                    if pk.cred_id() == res.cred_id() {
                        if pk.update_credential( &res).unwrap_or(false) {
                            let json = serde_json::to_string( &pk).map_err( |e| AuthError::Internal( e.to_string()))?;
                            self.store.update_credential( rec.id, &json, now_secs()).await?;
                        }
                        break;
                    }
                }
                true
            }
            Err(_) => false
        };
        self.conclude_login( user.as_ref(), verified, &keys, remote).await
    }

    async fn passkeys (&self, user_id: i64)->AuthResult<Vec<(CredentialRecord,Passkey)>> {
        Ok( self.store.credentials( user_id, Some(CredentialKind::Passkey)).await?.into_iter()
            .filter_map( |c| serde_json::from_str::<Passkey>( &c.data).ok().map( |pk| (c, pk))).collect())
    }

    /* #endregion login */

    /* #region enrollment ****************************************************************************************/

    /// determine what we enroll for - either a valid invite token or a logged in user (adding credentials)
    pub async fn enroll_target (&self, invite_token: Option<&str>, user: Option<&ConnUser>)->AuthResult<EnrollTarget> {
        let now = now_secs();
        if let Some(token) = invite_token.filter( |t| !t.is_empty()) {
            let token_hash = hash_token( token);
            let user_id = self.store.invite_user( &token_hash, now).await?.ok_or( AuthError::InvalidToken)?;
            let user = self.store.user( user_id).await?.filter( |u| u.is_active( now)).ok_or( AuthError::InvalidToken)?;
            Ok( EnrollTarget::Invite{ token_hash, user_id, user_name: user.name })
        } else if let Some(user) = user {
            Ok( EnrollTarget::User{ user_id: user.user_id, user_name: user.name.to_string() })
        } else {
            Err( AuthError::NotAuthenticated)
        }
    }

    /// store a new credential and conclude enrollment. Invite tokens are consumed and the user is logged in
    async fn add_enrolled_credential (&self, target: &EnrollTarget, kind: CredentialKind, label: Option<&str>, data: &str, remote: Option<&str>)->AuthResult<Option<LoginResult>> {
        let now = now_secs();
        if let EnrollTarget::Invite{ token_hash, .. } = target {
            if !self.store.remove_invite( token_hash).await? { return Err( AuthError::InvalidToken) }
        }
        self.store.add_credential( target.user_id(), kind, label, data, now).await?;

        match target {
            EnrollTarget::Invite{ user_id, .. } => {
                let user = self.store.user( *user_id).await?.ok_or( AuthError::InvalidToken)?;
                Ok( Some( self.create_session( &user, remote).await?))
            }
            EnrollTarget::User{..} => Ok(None)
        }
    }

    pub async fn passkey_enroll_start (&self, target: EnrollTarget, label: Option<String>)->AuthResult<(String,CreationChallengeResponse)> {
        let webauthn = self.webauthn()?;
        let user = self.store.user( target.user_id()).await?.ok_or( AuthError::InvalidToken)?;
        let uuid = Uuid::parse_str( &user.webauthn_id).map_err( |e| AuthError::Internal( e.to_string()))?;
        let exclude: Vec<_> = self.passkeys( user.id).await?.iter().map( |(_,pk)| pk.cred_id().clone()).collect();

        let (ccr, state) = webauthn.start_passkey_registration( uuid, &user.name, &user.name, Some(exclude))
            .map_err( |e| AuthError::WebAuthn( e.to_string()))?;
        let id = self.add_ceremony( CeremonyKind::PasskeyReg{ target, state, label })?;
        Ok( (id, ccr) )
    }

    pub async fn passkey_enroll_finish (&self, ceremony: &str, credential: &RegisterPublicKeyCredential, remote: Option<&str>)->AuthResult<Option<LoginResult>> {
        let webauthn = self.webauthn()?;
        let CeremonyKind::PasskeyReg{ target, state, label } = self.take_ceremony( ceremony)? else { return Err( AuthError::UnknownCeremony) };
        let pk = webauthn.finish_passkey_registration( credential, &state).map_err( |e| AuthError::WebAuthn( e.to_string()))?;
        let json = serde_json::to_string( &pk).map_err( |e| AuthError::Internal( e.to_string()))?;
        self.add_enrolled_credential( &target, CredentialKind::Passkey, Some( label.as_deref().unwrap_or("passkey")), &json, remote).await
    }

    pub async fn totp_enroll_start (&self, target: EnrollTarget)->AuthResult<TotpEnrollment> {
        self.check_available( AuthMethod::Totp)?;
        let mut secret = vec![0u8; 20];
        getrandom::fill( &mut secret).map_err( |e| AuthError::Internal( e.to_string()))?;
        let totp = build_totp( secret.clone(), &self.config.rp_name, target.user_name())?;
        let url = totp.to_url().map_err( |e| AuthError::Internal( e.to_string()))?;
        let qr_png = totp.to_qr_base64().map_err( |e| AuthError::Internal( e.to_string()))?;
        let secret_b32 = totp.secret().to_base32();
        let ceremony = self.add_ceremony( CeremonyKind::TotpReg{ target, secret, attempts: 0 })?;
        Ok( TotpEnrollment{ ceremony, url, qr_png, secret: secret_b32 })
    }

    /// verify the first code of a new TOTP authenticator. Wrong codes can be retried (within the ceremony TTL)
    pub async fn totp_enroll_finish (&self, ceremony: &str, code: &str, label: Option<&str>, remote: Option<&str>)->AuthResult<Option<LoginResult>> {
        let CeremonyKind::TotpReg{ target, secret, attempts } = self.take_ceremony( ceremony)? else { return Err( AuthError::UnknownCeremony) };
        let totp = build_totp( secret.clone(), &self.config.rp_name, target.user_name())?;
        match totp.check( code.trim(), now_secs() as u64) {
            Some(step) => {
                let data = TotpData{ secret: totp.secret().to_base32(), last_step: step };
                let json = serde_json::to_string( &data).map_err( |e| AuthError::Internal( e.to_string()))?;
                self.add_enrolled_credential( &target, CredentialKind::Totp, Some( label.unwrap_or("totp")), &json, remote).await
            }
            None => {
                if attempts + 1 < MAX_TOTP_ENROLL_ATTEMPTS { // allow retry with the same secret
                    let mut state = self.state.lock().unwrap();
                    state.ceremonies.insert( ceremony.to_string(), Ceremony{ kind: CeremonyKind::TotpReg{ target, secret, attempts: attempts + 1 }, expires: Instant::now() + CEREMONY_TTL });
                }
                Err( AuthError::InvalidCredentials)
            }
        }
    }

    /// set the password of the enroll target (replacing an existing one)
    pub async fn password_enroll (&self, target: EnrollTarget, password: &str, remote: Option<&str>)->AuthResult<Option<LoginResult>> {
        self.check_available( AuthMethod::Password)?;
        let hash = self.hash_password( password).await?;
        if let EnrollTarget::User{ user_id, .. } = &target {
            self.store.remove_credentials( *user_id, Some(CredentialKind::Password)).await?;
        }
        self.add_enrolled_credential( &target, CredentialKind::Password, Some("password"), &hash, remote).await
    }

    async fn hash_password (&self, password: &str)->AuthResult<String> {
        if password.chars().count() < self.config.min_password_len {
            return Err( AuthError::InvalidInput( format!("password has to have at least {} characters", self.config.min_password_len)))
        }
        let password = password.to_string();
        tokio::task::spawn_blocking( move || password_auth::generate_hash( password)).await.map_err( |e| AuthError::Internal( e.to_string()))
    }

    /* #endregion enrollment */

    /* #region administration ************************************************************************************/

    /// create a new user with given (configured) roles
    pub async fn create_user<S: AsRef<str>> (&self, name: &str, roles: &[S], expires: Option<i64>)->AuthResult<UserRecord> {
        if !is_valid_user_name( name) { return Err( AuthError::InvalidInput( format!("invalid user name '{name}'"))) }
        let rs = self.role_map.role_set( roles)?;
        let csv = self.role_map.names_of( rs).join(",");
        let uuid = random_uuid()?;
        let id = self.store.add_user( name, &csv, expires, &uuid.to_string(), now_secs()).await?;
        self.store.user( id).await?.ok_or( AuthError::Internal("user not stored".into()))
    }

    /// create an enrollment token for an existing user. Returns the token (to be sent to the user, e.g. as part of an
    /// enrollment URL) and its expiration (epoch secs)
    pub async fn create_invite (&self, name: &str)->AuthResult<(String,i64)> {
        let user = self.store.user_by_name( name).await?.ok_or_else( || AuthError::InvalidInput( format!("unknown user '{name}'")))?;
        let token = random_token( TOKEN_BYTES)?;
        let expires = now_secs() + self.config.invite_ttl.as_secs() as i64;
        self.store.add_invite( &hash_token( &token), user.id, expires).await?;
        Ok( (token, expires) )
    }

    pub async fn set_roles<S: AsRef<str>> (&self, name: &str, roles: &[S])->AuthResult<()> {
        let rs = self.role_map.role_set( roles)?;
        if self.store.set_roles( name, &self.role_map.names_of( rs).join(",")).await? { Ok(()) } else { Err( AuthError::InvalidInput( format!("unknown user '{name}'"))) }
    }

    /// set the password of a user (admin function, e.g. to bootstrap the first admin)
    pub async fn set_password (&self, name: &str, password: &str)->AuthResult<()> {
        let user = self.store.user_by_name( name).await?.ok_or_else( || AuthError::InvalidInput( format!("unknown user '{name}'")))?;
        let hash = self.hash_password( password).await?;
        self.store.remove_credentials( user.id, Some(CredentialKind::Password)).await?;
        self.store.add_credential( user.id, CredentialKind::Password, Some("password"), &hash, now_secs()).await?;
        Ok(())
    }

    /* #endregion administration */
}

fn create_webauthn (config: &AuthConfig, origins: &[&String])->AuthResult<Webauthn> {
    let urls: Vec<Url> = origins.iter().map( |o| Url::parse( o).map_err( |e| AuthError::Config( format!("invalid origin '{o}': {e}")))).collect::<AuthResult<_>>()?;
    let mut builder = WebauthnBuilder::new( &config.rp_id, &urls[0]).map_err( |e| AuthError::Config( format!("invalid WebAuthn config: {e:?}")))?
        .rp_name( &config.rp_name)
        .allow_any_port( config.rp_id == "localhost");
    for url in &urls[1..] { builder = builder.append_allowed_origin( url); }
    builder.build().map_err( |e| AuthError::Config( format!("invalid WebAuthn config: {e:?}")))
}

fn build_totp (secret: Vec<u8>, issuer: &str, account: &str)->AuthResult<Totp> {
    TotpBuilder::new()
        .with_algorithm( Algorithm::SHA1) // most widely supported by authenticator apps
        .with_digits(6)
        .with_skew(1)
        .with_step_duration(30)
        .with_secret( secret)
        .with_issuer( Some(issuer))
        .with_account_name( account)
        .build()
        .map_err( |e| AuthError::Internal( format!("TOTP error: {e}")))
}
