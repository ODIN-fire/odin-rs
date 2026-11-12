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

//! persistent SQLite store for users, credentials, enrollment invites and sessions.
//! Secrets are never stored in clear: session and invite tokens are stored as SHA-256 hashes, passwords as argon2
//! hashes. TOTP secrets have to be stored as they are (they are required to verify codes), hence the database file is
//! created with owner-only permissions.

use std::{path::Path, time::Duration};
use sqlx::{Row, SqlitePool, sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow}};
use super::{AuthError, AuthResult};

impl From<sqlx::Error> for AuthError {
    fn from (e: sqlx::Error)->Self { AuthError::Db( e.to_string()) }
}

const SCHEMA: &[&'static str] = &[
    r#"create table if not exists users (
        id integer primary key not null,
        name text not null unique,
        roles text not null default '',
        created integer not null,
        expires integer,
        disabled integer not null default 0,
        epoch integer not null default 0,
        webauthn_id text not null
    )"#,
    r#"create table if not exists credentials (
        id integer primary key not null,
        user_id integer not null references users(id) on delete cascade,
        kind text not null,
        label text,
        data text not null,
        created integer not null,
        last_used integer
    )"#,
    r#"create table if not exists invites (
        token_hash text primary key not null,
        user_id integer not null references users(id) on delete cascade,
        expires integer not null
    )"#,
    r#"create table if not exists sessions (
        token_hash text primary key not null,
        user_id integer not null references users(id) on delete cascade,
        epoch integer not null,
        created integer not null,
        expires integer not null,
        remote text
    )"#,
];

#[derive(Debug,Clone,PartialEq)]
pub struct UserRecord {
    pub id: i64,
    pub name: String,
    pub roles: String,          // comma separated role names
    pub created: i64,           // epoch secs
    pub expires: Option<i64>,   // epoch secs, None if account does not expire
    pub disabled: bool,
    pub epoch: i64,             // incremented to invalidate all sessions of this user
    pub webauthn_id: String,    // random UUID used as WebAuthn user handle
}

impl UserRecord {
    fn from_row (row: &SqliteRow)->AuthResult<Self> {
        Ok( UserRecord {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            roles: row.try_get("roles")?,
            created: row.try_get("created")?,
            expires: row.try_get("expires")?,
            disabled: row.try_get::<i64,_>("disabled")? != 0,
            epoch: row.try_get("epoch")?,
            webauthn_id: row.try_get("webauthn_id")?,
        })
    }

    pub fn is_active (&self, now: i64)->bool {
        !self.disabled && self.expires.map( |e| e > now).unwrap_or(true)
    }
}

#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub enum CredentialKind { Passkey, Totp, Password }

impl CredentialKind {
    pub fn as_str (&self)->&'static str {
        match self { CredentialKind::Passkey => "passkey", CredentialKind::Totp => "totp", CredentialKind::Password => "password" }
    }
    pub fn parse (s: &str)->AuthResult<Self> {
        match s {
            "passkey" => Ok(CredentialKind::Passkey),
            "totp" => Ok(CredentialKind::Totp),
            "password" => Ok(CredentialKind::Password),
            _ => Err( AuthError::InvalidInput( format!("unknown credential kind '{s}'")))
        }
    }
}

#[derive(Debug,Clone)]
pub struct CredentialRecord {
    pub id: i64,
    pub user_id: i64,
    pub kind: CredentialKind,
    pub label: Option<String>,
    pub data: String,           // kind specific (passkey JSON, TOTP JSON, password hash)
    pub created: i64,
    pub last_used: Option<i64>,
}

impl CredentialRecord {
    fn from_row (row: &SqliteRow)->AuthResult<Self> {
        Ok( CredentialRecord {
            id: row.try_get("id")?,
            user_id: row.try_get("user_id")?,
            kind: CredentialKind::parse( row.try_get::<&str,_>("kind")?)?,
            label: row.try_get("label")?,
            data: row.try_get("data")?,
            created: row.try_get("created")?,
            last_used: row.try_get("last_used")?,
        })
    }
}

#[derive(Debug,Clone)]
pub struct SessionRecord {
    pub token_hash: String,
    pub user_id: i64,
    pub epoch: i64,
    pub created: i64,
    pub expires: i64,
    pub remote: Option<String>,
}

impl SessionRecord {
    fn from_row (row: &SqliteRow)->AuthResult<Self> {
        Ok( SessionRecord {
            token_hash: row.try_get("token_hash")?,
            user_id: row.try_get("user_id")?,
            epoch: row.try_get("epoch")?,
            created: row.try_get("created")?,
            expires: row.try_get("expires")?,
            remote: row.try_get("remote")?,
        })
    }
}

/// the persistent auth data store
#[derive(Debug,Clone)]
pub struct AuthStore {
    pool: SqlitePool,
}

impl AuthStore {
    /// open (and if required create) the database at `path`
    pub async fn open (path: &Path)->AuthResult<Self> {
        if let Some(dir) = path.parent() { std::fs::create_dir_all( dir).map_err( |e| AuthError::Db( format!("cannot create {dir:?}: {e}")))?; }
        let is_new = !path.exists();
        let options = SqliteConnectOptions::new()
            .filename( path)
            .create_if_missing( true)
            .foreign_keys( true)
            .journal_mode( SqliteJournalMode::Wal)
            .busy_timeout( Duration::from_secs(5));
        let pool = SqlitePoolOptions::new().max_connections( 4).connect_with( options).await?;
        if is_new { restrict_permissions( path); }
        let store = AuthStore{ pool };
        store.create_schema().await?;
        Ok(store)
    }

    /// in-memory database (for tests)
    pub async fn open_memory ()->AuthResult<Self> {
        let options = SqliteConnectOptions::new().in_memory( true).foreign_keys( true);
        // each connection of an in-memory db is a separate db, hence we only use one connection that never expires
        let pool = SqlitePoolOptions::new().max_connections(1).min_connections(1).idle_timeout(None).max_lifetime(None)
            .connect_with( options).await?;
        let store = AuthStore{ pool };
        store.create_schema().await?;
        Ok(store)
    }

    async fn create_schema (&self)->AuthResult<()> {
        for stmt in SCHEMA {
            sqlx::query( *stmt).execute( &self.pool).await?;
        }
        Ok(())
    }

    //--- users

    pub async fn add_user (&self, name: &str, roles: &str, expires: Option<i64>, webauthn_id: &str, now: i64)->AuthResult<i64> {
        let res = sqlx::query( "insert into users (name, roles, created, expires, webauthn_id) values (?,?,?,?,?)")
            .bind( name).bind( roles).bind( now).bind( expires).bind( webauthn_id)
            .execute( &self.pool).await;
        match res {
            Ok(r) => Ok( r.last_insert_rowid()),
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => Err( AuthError::InvalidInput( format!("user '{name}' already exists"))),
            Err(e) => Err( e.into())
        }
    }

    pub async fn user_by_name (&self, name: &str)->AuthResult<Option<UserRecord>> {
        let row = sqlx::query( "select * from users where name = ?").bind( name).fetch_optional( &self.pool).await?;
        row.map( |r| UserRecord::from_row( &r)).transpose()
    }

    pub async fn user (&self, id: i64)->AuthResult<Option<UserRecord>> {
        let row = sqlx::query( "select * from users where id = ?").bind( id).fetch_optional( &self.pool).await?;
        row.map( |r| UserRecord::from_row( &r)).transpose()
    }

    pub async fn users (&self)->AuthResult<Vec<UserRecord>> {
        let rows = sqlx::query( "select * from users order by name").fetch_all( &self.pool).await?;
        rows.iter().map( UserRecord::from_row).collect()
    }

    pub async fn set_roles (&self, name: &str, roles: &str)->AuthResult<bool> {
        Ok( sqlx::query( "update users set roles = ? where name = ?").bind( roles).bind( name).execute( &self.pool).await?.rows_affected() > 0)
    }

    pub async fn set_expires (&self, name: &str, expires: Option<i64>)->AuthResult<bool> {
        Ok( sqlx::query( "update users set expires = ? where name = ?").bind( expires).bind( name).execute( &self.pool).await?.rows_affected() > 0)
    }

    /// disabling a user also invalidates all its sessions
    pub async fn set_disabled (&self, name: &str, disabled: bool)->AuthResult<bool> {
        Ok( sqlx::query( "update users set disabled = ?, epoch = epoch + 1 where name = ?")
            .bind( disabled as i64).bind( name).execute( &self.pool).await?.rows_affected() > 0)
    }

    /// invalidate all sessions of a user
    pub async fn bump_epoch (&self, name: &str)->AuthResult<bool> {
        let n = sqlx::query( "update users set epoch = epoch + 1 where name = ?").bind( name).execute( &self.pool).await?.rows_affected();
        sqlx::query( "delete from sessions where user_id = (select id from users where name = ?)").bind( name).execute( &self.pool).await?;
        Ok( n > 0)
    }

    /// remove user and all related credentials, invites and sessions
    pub async fn remove_user (&self, name: &str)->AuthResult<bool> {
        Ok( sqlx::query( "delete from users where name = ?").bind( name).execute( &self.pool).await?.rows_affected() > 0)
    }

    //--- credentials

    pub async fn add_credential (&self, user_id: i64, kind: CredentialKind, label: Option<&str>, data: &str, now: i64)->AuthResult<i64> {
        let r = sqlx::query( "insert into credentials (user_id, kind, label, data, created) values (?,?,?,?,?)")
            .bind( user_id).bind( kind.as_str()).bind( label).bind( data).bind( now)
            .execute( &self.pool).await?;
        Ok( r.last_insert_rowid())
    }

    pub async fn credentials (&self, user_id: i64, kind: Option<CredentialKind>)->AuthResult<Vec<CredentialRecord>> {
        let rows = match kind {
            Some(kind) => sqlx::query( "select * from credentials where user_id = ? and kind = ? order by id")
                .bind( user_id).bind( kind.as_str()).fetch_all( &self.pool).await?,
            None => sqlx::query( "select * from credentials where user_id = ? order by id")
                .bind( user_id).fetch_all( &self.pool).await?,
        };
        rows.iter().map( CredentialRecord::from_row).collect()
    }

    pub async fn update_credential (&self, id: i64, data: &str, last_used: i64)->AuthResult<()> {
        sqlx::query( "update credentials set data = ?, last_used = ? where id = ?").bind( data).bind( last_used).bind( id).execute( &self.pool).await?;
        Ok(())
    }

    pub async fn remove_credentials (&self, user_id: i64, kind: Option<CredentialKind>)->AuthResult<u64> {
        let r = match kind {
            Some(kind) => sqlx::query( "delete from credentials where user_id = ? and kind = ?").bind( user_id).bind( kind.as_str()).execute( &self.pool).await?,
            None => sqlx::query( "delete from credentials where user_id = ?").bind( user_id).execute( &self.pool).await?,
        };
        Ok( r.rows_affected())
    }

    //--- invites

    pub async fn add_invite (&self, token_hash: &str, user_id: i64, expires: i64)->AuthResult<()> {
        sqlx::query( "insert into invites (token_hash, user_id, expires) values (?,?,?)").bind( token_hash).bind( user_id).bind( expires).execute( &self.pool).await?;
        Ok(())
    }

    /// user id of a valid (unexpired) invite
    pub async fn invite_user (&self, token_hash: &str, now: i64)->AuthResult<Option<i64>> {
        let row = sqlx::query( "select user_id from invites where token_hash = ? and expires > ?").bind( token_hash).bind( now).fetch_optional( &self.pool).await?;
        Ok( row.map( |r| r.try_get("user_id")).transpose()?)
    }

    pub async fn remove_invite (&self, token_hash: &str)->AuthResult<bool> {
        Ok( sqlx::query( "delete from invites where token_hash = ?").bind( token_hash).execute( &self.pool).await?.rows_affected() > 0)
    }

    pub async fn remove_user_invites (&self, user_id: i64)->AuthResult<u64> {
        Ok( sqlx::query( "delete from invites where user_id = ?").bind( user_id).execute( &self.pool).await?.rows_affected())
    }

    //--- sessions

    pub async fn add_session (&self, s: &SessionRecord)->AuthResult<()> {
        sqlx::query( "insert into sessions (token_hash, user_id, epoch, created, expires, remote) values (?,?,?,?,?,?)")
            .bind( &s.token_hash).bind( s.user_id).bind( s.epoch).bind( s.created).bind( s.expires).bind( &s.remote)
            .execute( &self.pool).await?;
        Ok(())
    }

    pub async fn session (&self, token_hash: &str)->AuthResult<Option<SessionRecord>> {
        let row = sqlx::query( "select * from sessions where token_hash = ?").bind( token_hash).fetch_optional( &self.pool).await?;
        row.map( |r| SessionRecord::from_row( &r)).transpose()
    }

    pub async fn remove_session (&self, token_hash: &str)->AuthResult<bool> {
        Ok( sqlx::query( "delete from sessions where token_hash = ?").bind( token_hash).execute( &self.pool).await?.rows_affected() > 0)
    }

    pub async fn user_sessions (&self, user_id: i64)->AuthResult<Vec<SessionRecord>> {
        let rows = sqlx::query( "select * from sessions where user_id = ? order by created").bind( user_id).fetch_all( &self.pool).await?;
        rows.iter().map( SessionRecord::from_row).collect()
    }

    /// remove expired sessions and invites
    pub async fn purge_expired (&self, now: i64)->AuthResult<u64> {
        let n = sqlx::query( "delete from sessions where expires <= ?").bind( now).execute( &self.pool).await?.rows_affected();
        let m = sqlx::query( "delete from invites where expires <= ?").bind( now).execute( &self.pool).await?.rows_affected();
        Ok( n + m)
    }
}

#[cfg(unix)]
fn restrict_permissions (path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions( path, std::fs::Permissions::from_mode( 0o600));
}

#[cfg(not(unix))]
fn restrict_permissions (_path: &Path) {}
