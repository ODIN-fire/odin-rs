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

// note: role, message ACL and config tests do not require the `auth` feature, Authenticator tests do

use std::time::Duration;
use odin_common::auth::*;

/* #region roles, ACLs and config ***************************************************************************/

#[test]
fn test_roles () {
    let rm = RoleMap::new( &["viewer", "editor", "admin"]).unwrap();
    let viewer = rm.role_set( &["viewer"]).unwrap();
    let editor_admin = rm.role_set( &["editor", "admin"]).unwrap();
    assert!( viewer.satisfies( RoleSet::NONE));
    assert!( !viewer.satisfies( editor_admin));
    assert!( rm.role_set( &["admin"]).unwrap().satisfies( editor_admin));
    assert!( RoleSet::ALL.satisfies( editor_admin));
    assert!( !RoleSet::NONE.satisfies( viewer));
    assert_eq!( rm.names_of( editor_admin), vec!["editor", "admin"]);
    assert_eq!( rm.parse_lenient( "admin, gone ,viewer"), rm.role_set( &["viewer", "admin"]).unwrap());
    assert!( rm.role_set( &["gone"]).is_err());
    assert!( RoleMap::new( &["a", "a"]).is_err());
    assert!( RoleMap::new( &["a b"]).is_err());
    let many: Vec<String> = (0..65).map( |i| format!("r{i}")).collect();
    assert!( RoleMap::new( &many).is_err());
    assert!( RoleMap::new( &many[..64]).unwrap().role_set( &["r63"]).unwrap().contains(63));
}

#[test]
fn test_msg_acl () {
    let rm = RoleMap::new( &["viewer", "editor"]).unwrap();
    let mut acl = MsgAcl::new();
    acl.set( "odin_share/odin_share.js:setShared", rm.role_set( &["editor"]).unwrap());
    acl.set( "odin_x/x.js", rm.role_set( &["viewer"]).unwrap());
    assert_eq!( acl.required( "odin_share/odin_share.js", "setShared"), Some( rm.role_set( &["editor"]).unwrap()));
    assert_eq!( acl.required( "odin_share/odin_share.js", "getShared"), None);
    assert_eq!( acl.required( "odin_x/x.js", "anything"), Some( rm.role_set( &["viewer"]).unwrap()));
    assert!( is_authorized( None, RoleSet::ALL));
}

#[test]
fn test_config () {
    let cfg: AuthConfig = ron::from_str( r#"AuthConfig( roles: ["viewer","editor"], session_ttl: "8h", ws_msg_roles: { "m.js:x": ["editor"] } )"#).unwrap();
    assert_eq!( cfg.session_ttl, Duration::from_secs( 8*3600));
    assert_eq!( cfg.methods, vec![AuthMethod::Passkey, AuthMethod::Totp]);
    assert!( cfg.protect_proxies);
    assert!( cfg.validate().is_ok());
    let mut bad = cfg.clone();
    bad.ws_msg_roles.insert( "m.js:y".into(), vec!["nobody".into()]);
    assert!( bad.validate().is_err());
    let mut ip = cfg.clone();
    ip.rp_id = "192.168.1.10".into();
    assert!( ip.validate().is_err());
}

/* #endregion roles, ACLs and config */

/* #region Authenticator (requires 'auth' feature) **********************************************************/

#[cfg(feature="auth")]
mod authenticator {
    use super::*;
    use totp_rs::{Builder, Secret};
    use webauthn_authenticator_rs::{softpasskey::SoftPasskey, WebauthnAuthenticator};
    use url::Url;

    const ORIGIN: &str = "http://localhost:9009";

    async fn authenticator (methods: Vec<AuthMethod>)->Authenticator {
        let mut config = AuthConfig::with_roles( &["viewer", "editor", "admin"]);
        config.methods = methods;
        config.max_failures = 3;
        config.lockout = Duration::from_secs(60);
        config.min_password_len = 8;
        let store = AuthStore::open_memory().await.unwrap();
        Authenticator::with_store( config, store, &[ORIGIN.to_string()]).unwrap()
    }

    fn totp_code (secret_b32: &str, user: &str, time: u64)->String {
        let secret = Secret::try_from_base32( secret_b32).unwrap();
        Builder::new().with_secret( secret.as_bytes().to_vec()).with_issuer( Some("ODIN")).with_account_name( user).build().unwrap()
            .generate( time).to_string()
    }

    fn now ()->u64 { now_secs() as u64 }

    #[tokio::test]
    async fn test_password_and_sessions () {
        let auth = authenticator( vec![AuthMethod::Password]).await;
        let user = auth.create_user( "alice", &["editor"], None).await.unwrap();
        assert_eq!( user.roles, "editor");
        assert!( auth.create_user( "alice", &["viewer"], None).await.is_err()); // duplicate
        assert!( auth.create_user( "bad name", &["viewer"], None).await.is_err());
        assert!( auth.create_user( "bob", &["nobody"], None).await.is_err()); // unknown role

        // enroll with invite (logs in and consumes the invite)
        let (token, _) = auth.create_invite( "alice").await.unwrap();
        let target = auth.enroll_target( Some(&token), None).await.unwrap();
        assert_eq!( target.user_name(), "alice");
        assert!( matches!( auth.password_enroll( target.clone(), "short", None).await, Err(AuthError::InvalidInput(_))));
        let login = auth.password_enroll( target, "correct horse", None).await.unwrap().expect("invite enrollment logs in");
        assert_eq!( login.user.name.as_ref(), "alice");
        assert!( auth.enroll_target( Some(&token), None).await.is_err()); // invite consumed

        // session lookup
        let u = auth.session_user( &login.token).await.expect("valid session");
        assert_eq!( u.roles, auth.role_map().role_set( &["editor"]).unwrap());
        assert!( u.is_authorized( auth.role_map().role_set( &["editor","admin"]).unwrap()));
        assert!( !u.is_authorized( auth.role_map().role_set( &["admin"]).unwrap()));
        assert!( auth.session_user( "bogus").await.is_none());

        // login
        assert_eq!( auth.login_password( "alice", "wrong password", Some("1.2.3.4")).await.unwrap_err(), AuthError::InvalidCredentials);
        assert_eq!( auth.login_password( "nobody", "correct horse", None).await.unwrap_err(), AuthError::InvalidCredentials);
        let login2 = auth.login_password( "alice", "correct horse", Some("1.2.3.4")).await.unwrap();
        assert_ne!( login.token, login2.token);

        // role change and revocation are picked up by refresh/revalidate
        assert_eq!( auth.revalidate( &u), Revalidation::Valid);
        auth.set_roles( "alice", &["admin"]).await.unwrap();
        auth.refresh().await.unwrap();
        assert_eq!( auth.revalidate( &u), Revalidation::RolesChanged( auth.role_map().role_set( &["admin"]).unwrap()));
        assert!( auth.store().bump_epoch( "alice").await.unwrap());
        auth.refresh().await.unwrap();
        assert_eq!( auth.revalidate( &u), Revalidation::Invalid);
        assert!( auth.session_user( &login2.token).await.is_none());

        // logout
        let login3 = auth.login_password( "alice", "correct horse", None).await.unwrap();
        assert!( auth.session_user( &login3.token).await.is_some());
        assert_eq!( auth.logout( &login3.token).await.unwrap().as_deref(), Some( login3.user.session.as_ref()));
        assert!( auth.session_user( &login3.token).await.is_none());

        // disabled users can't log in
        auth.store().set_disabled( "alice", true).await.unwrap();
        assert!( auth.login_password( "alice", "correct horse", None).await.is_err());

        // methods not enabled
        assert!( matches!( auth.login_totp( "alice", "123456", None).await, Err(AuthError::MethodNotEnabled(_))));
    }

    #[tokio::test]
    async fn test_lockout () {
        let auth = authenticator( vec![AuthMethod::Password]).await;
        auth.create_user( "carol", &["viewer"], None).await.unwrap();
        auth.set_password( "carol", "carols password").await.unwrap();
        for _ in 0..3 {
            assert_eq!( auth.login_password( "carol", "guess", Some("10.0.0.1")).await.unwrap_err(), AuthError::InvalidCredentials);
        }
        // locked for user name (even from other addresses and with correct password)
        assert!( matches!( auth.login_password( "carol", "carols password", Some("10.0.0.2")).await, Err(AuthError::Locked(_))));
        // and for the address (even for other users)
        assert!( matches!( auth.login_password( "dave", "x", Some("10.0.0.1")).await, Err(AuthError::Locked(_))));
    }

    #[tokio::test]
    async fn test_totp () {
        let auth = authenticator( vec![AuthMethod::Totp]).await;
        auth.create_user( "erin", &["viewer"], None).await.unwrap();
        let (token, _) = auth.create_invite( "erin").await.unwrap();
        let target = auth.enroll_target( Some(&token), None).await.unwrap();

        let enr = auth.totp_enroll_start( target).await.unwrap();
        assert!( enr.url.starts_with("otpauth://totp/ODIN:erin?"));
        assert!( enr.qr_png.len() > 100);

        // a wrong code can be retried
        let t = now();
        let code = totp_code( &enr.secret, "erin", t);
        let wrong = if code == "000000" { "111111" } else { "000000" };
        assert_eq!( auth.totp_enroll_finish( &enr.ceremony, wrong, None, None).await.unwrap_err(), AuthError::InvalidCredentials);
        let login = auth.totp_enroll_finish( &enr.ceremony, &code, None, None).await.unwrap().expect("logged in");
        assert_eq!( login.user.name.as_ref(), "erin");

        // the enrollment code can't be replayed, the next step's code works (skew 1)
        assert_eq!( auth.login_totp( "erin", &code, None).await.unwrap_err(), AuthError::InvalidCredentials);
        let next = totp_code( &enr.secret, "erin", t + 30);
        assert!( auth.login_totp( "erin", &next, None).await.is_ok());
        assert_eq!( auth.login_totp( "erin", &next, None).await.unwrap_err(), AuthError::InvalidCredentials);

        // logged in user can add another authenticator
        let target = auth.enroll_target( None, Some( &login.user)).await.unwrap();
        let enr2 = auth.totp_enroll_start( target).await.unwrap();
        let code2 = totp_code( &enr2.secret, "erin", now());
        assert!( auth.totp_enroll_finish( &enr2.ceremony, &code2, Some("phone 2"), None).await.unwrap().is_none());
        let user = auth.store().user_by_name("erin").await.unwrap().unwrap();
        assert_eq!( auth.store().credentials( user.id, Some(CredentialKind::Totp)).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_passkey () {
        let auth = authenticator( vec![AuthMethod::Passkey, AuthMethod::Totp]).await;
        assert_eq!( auth.available_methods(), vec![AuthMethod::Passkey, AuthMethod::Totp]);
        auth.create_user( "frank", &["admin"], None).await.unwrap();
        let (token, _) = auth.create_invite( "frank").await.unwrap();
        let origin = Url::parse( ORIGIN).unwrap();
        let mut authenticator = SoftPasskey::new( true);

        // registration
        let target = auth.enroll_target( Some(&token), None).await.unwrap();
        let (ceremony, ccr) = auth.passkey_enroll_start( target, Some("laptop".into())).await.unwrap();
        let ccr: CreationChallengeResponse = serde_json::from_str( &serde_json::to_string( &ccr).unwrap()).unwrap(); // goes through JSON
        let reg = authenticator.do_registration( origin.clone(), ccr).unwrap();
        let login = auth.passkey_enroll_finish( &ceremony, &reg, None).await.unwrap().expect("logged in");
        assert_eq!( login.user.name.as_ref(), "frank");
        assert!( auth.passkey_enroll_finish( &ceremony, &reg, None).await.is_err()); // ceremony consumed

        // authentication
        let (ceremony, rcr) = auth.passkey_login_start( "frank", None).await.unwrap();
        let cred = authenticator.do_authentication( origin.clone(), rcr).unwrap();
        let login = auth.passkey_login_finish( &ceremony, &cred, Some("127.0.0.1")).await.unwrap();
        assert!( auth.session_user( &login.token).await.is_some());

        // unknown user and wrong authenticator fail
        assert_eq!( auth.passkey_login_start( "nobody", None).await.unwrap_err(), AuthError::InvalidCredentials);
        let (_ceremony, rcr) = auth.passkey_login_start( "frank", None).await.unwrap();
        let mut other = SoftPasskey::new( true);
        assert!( other.do_authentication( origin.clone(), rcr).is_err()); // has no credential for this user

        // a response for another ceremony is rejected
        let (c1, rcr1) = auth.passkey_login_start( "frank", None).await.unwrap();
        let (_c2, rcr2) = auth.passkey_login_start( "frank", None).await.unwrap();
        let _ = rcr1;
        let cred2 = authenticator.do_authentication( origin.clone(), rcr2).unwrap();
        assert_eq!( auth.passkey_login_finish( &c1, &cred2, None).await.unwrap_err(), AuthError::InvalidCredentials);

        // no passkeys without origins (admin tools)
        let cli = Authenticator::with_store( auth.config().clone(), AuthStore::open_memory().await.unwrap(), &[]).unwrap();
        assert_eq!( cli.available_methods(), vec![AuthMethod::Totp]);
    }
} // mod authenticator

/* #endregion Authenticator */
