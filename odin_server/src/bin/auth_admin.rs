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

//! auth_admin - user administration for ODIN servers with authentication (`auth` section in the server config).
//! Users are created with roles and get an enrollment URL (valid for the configured `invite_ttl`), which they use to
//! register their own credentials (passkey, TOTP authenticator app or password). Admins never handle user secrets
//! (except for the `--set-password` bootstrap option). This works directly on the user database, i.e. also while
//! servers are running (changes are picked up within 30 sec).

use std::{io::{BufRead, Write}, path::PathBuf};
use anyhow::{anyhow, Result};
use odin_common::{define_cli, auth::{Authenticator, CredentialKind, now_secs}};
use odin_server::ServerConfig;

define_cli! { ARGS [about="auth_admin - user administration for ODIN servers with authentication"] =
    config: Option<PathBuf> [help="server config file with 'auth' section (default: $ODIN_ROOT/configs/odin_server/spa_server.ron)", long],
    list: bool [help="list users", long, short],
    add_user: Option<String> [help="add user and print enrollment URL (requires --roles)", long],
    invite: Option<String> [help="print new enrollment URL for existing user (e.g. to add credentials)", long],
    roles: Option<String> [help="comma separated role names (for --add-user and --set-roles)", long],
    expires_days: Option<u64> [help="account expiration in days (for --add-user)", long],
    set_roles: Option<String> [help="set roles of user (requires --roles)", long],
    disable: Option<String> [help="disable user (ends all sessions)", long],
    enable: Option<String> [help="re-enable disabled user", long],
    revoke: Option<String> [help="end all sessions of user", long],
    remove_user: Option<String> [help="remove user including all credentials and sessions", long],
    remove_credentials: Option<String> [help="remove credentials of user (all or --kind)", long],
    kind: Option<String> [help="credential kind for --remove-credentials: passkey, totp or password", long],
    set_password: Option<String> [help="set password of user (read from stdin) - requires 'Password' method", long],
    app: Option<String> [help="application name for enrollment URLs (e.g. odin_live)", long],
    url: Option<String> [help="server base URL for enrollment URLs (default derived from config)", long]
}

#[tokio::main]
async fn main ()->Result<()> {
    odin_build::set_bin_context!();

    let server_config: ServerConfig = match &ARGS.config {
        Some(path) => ron::from_str( &std::fs::read_to_string( path)?)?,
        None => odin_server::load_config("spa_server.ron")?
    };
    let auth_config = server_config.auth.clone().ok_or_else( || anyhow!("server config has no 'auth' section"))?;
    let auth = Authenticator::new( auth_config, &[]).await?;

    if let Some(name) = &ARGS.add_user {
        let roles = role_list()?;
        let expires = ARGS.expires_days.map( |d| now_secs() + (d * 86400) as i64);
        auth.create_user( name, &roles, expires).await?;
        println!("added user '{name}' with roles {roles:?}");
        print_invite( &auth, &server_config, name).await?;
    }

    if let Some(name) = &ARGS.invite { print_invite( &auth, &server_config, name).await?; }

    if let Some(name) = &ARGS.set_roles {
        let roles = role_list()?;
        auth.set_roles( name, &roles).await?;
        println!("set roles of '{name}' to {roles:?}");
    }

    if let Some(name) = &ARGS.disable { check( auth.store().set_disabled( name, true).await?, name)?; println!("disabled '{name}'"); }
    if let Some(name) = &ARGS.enable { check( auth.store().set_disabled( name, false).await?, name)?; println!("enabled '{name}'"); }
    if let Some(name) = &ARGS.revoke { check( auth.store().bump_epoch( name).await?, name)?; println!("revoked all sessions of '{name}'"); }
    if let Some(name) = &ARGS.remove_user { check( auth.store().remove_user( name).await?, name)?; println!("removed '{name}'"); }

    if let Some(name) = &ARGS.remove_credentials {
        let user = auth.store().user_by_name( name).await?.ok_or_else( || anyhow!("unknown user '{name}'"))?;
        let kind = ARGS.kind.as_deref().map( CredentialKind::parse).transpose()?;
        let n = auth.store().remove_credentials( user.id, kind).await?;
        auth.store().bump_epoch( name).await?;
        println!("removed {n} credential(s) of '{name}' and ended its sessions");
    }

    if let Some(name) = &ARGS.set_password {
        print!("new password for '{name}' (will be echoed): ");
        std::io::stdout().flush()?;
        let mut pw = String::new();
        std::io::stdin().lock().read_line( &mut pw)?;
        auth.set_password( name, pw.trim_end_matches(['\n','\r'])).await?;
        println!("password set");
    }

    if ARGS.list { list_users( &auth).await?; }
    Ok(())
}

fn role_list ()->Result<Vec<String>> {
    let roles = ARGS.roles.as_ref().ok_or_else( || anyhow!("missing --roles"))?;
    Ok( roles.split(',').map( |s| s.trim().to_string()).filter( |s| !s.is_empty()).collect())
}

fn check (found: bool, name: &str)->Result<()> {
    if found { Ok(()) } else { Err( anyhow!("unknown user '{name}'")) }
}

async fn print_invite (auth: &Authenticator, config: &ServerConfig, name: &str)->Result<()> {
    let (token, expires) = auth.create_invite( name).await?;
    let base = ARGS.url.clone().unwrap_or_else( || base_url( config));
    let app = ARGS.app.as_deref().unwrap_or("<app>");
    println!("enrollment URL for '{name}' (valid until {}):", fmt_time( expires));
    println!("  {base}/{app}/auth/enroll?token={token}");
    Ok(())
}

/// best guess for the URL under which users reach the server
fn base_url (config: &ServerConfig)->String {
    let proto = if config.tls.is_some() { "https" } else { "http" };
    let ip = config.sock_addr.ip();
    let rp_id = config.auth.as_ref().map( |a| a.rp_id.as_str()).unwrap_or("localhost");
    let host = if ip.is_loopback() { "localhost".to_string() }
        else if rp_id != "localhost" { rp_id.to_string() }
        else if ip.is_unspecified() { "<host>".to_string() } // bound to all interfaces - use --url
        else { ip.to_string() };
    format!("{proto}://{host}:{}", config.sock_addr.port())
}

fn fmt_time (epoch_secs: i64)->String {
    chrono::DateTime::from_timestamp( epoch_secs, 0).map( |d| d.format("%Y-%m-%d %H:%M UTC").to_string()).unwrap_or_else( || epoch_secs.to_string())
}

async fn list_users (auth: &Authenticator)->Result<()> {
    let now = now_secs();
    let users = auth.store().users().await?;
    println!("{:<20} {:<24} {:<8} {:<20} {:<26} {}", "user", "roles", "status", "expires", "credentials", "sessions");
    for u in users {
        let status = if u.disabled { "disabled" } else if !u.is_active( now) { "expired" } else { "active" };
        let expires = u.expires.map( fmt_time).unwrap_or_else( || "-".into());
        let creds = auth.store().credentials( u.id, None).await?;
        let cred_list: Vec<String> = creds.iter().map( |c| c.label.clone().unwrap_or_else( || c.kind.as_str().to_string())).collect();
        let sessions = auth.store().user_sessions( u.id).await?.iter().filter( |s| s.expires > now && s.epoch == u.epoch).count();
        println!("{:<20} {:<24} {:<8} {:<20} {:<26} {}", u.name, u.roles, status, expires, cred_list.join(","), sessions);
    }
    Ok(())
}
