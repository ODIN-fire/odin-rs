/*
 * Copyright © 2025, United States Government, as represented by the Administrator of 
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

use std::{path::{Path,PathBuf}, fs::{File,read_to_string}, io::Write};
use odin_server::errors::OdinServerError;
use tokio;
use sqlx::{SqlitePool,SqliteConnection,Executor,sqlite::SqliteConnectOptions};
use passwords::PasswordGenerator;
use password_auth::generate_hash;
use chrono::{Utc,DateTime,Days};
use anyhow::{anyhow,Result};
use odin_common::{define_cli, fs::path_to_lossy_string};
use odin_server::errors::OdinServerResult;
use odin_server::auth::User;

define_cli! { ARGS [about="user database administration tool"] =
    create: bool [help="create database if not existent", long, short],
    list_all: bool [help="list database contents", long, short],
    migrate: Option<PathBuf> [help="write SQL code to create table and populate it with current values", long, short],
    add_user: Option<String> [help="add user name (fail if exists)", long, short],
    update_user: Option<String> [help="update user email, password or expiration date", long, short],
    email: Option<String> [help="user email", long, short],
    days_valid: Option<u64> [help="number of days valid", long, default_value="365"],
    password: Option<String> [help="password for new user", long, short],
    remove_user: Option<String> [help="remove user name", long, short],
    find_user: Option<String> [help="lookup user by name", long, short],
    write_to: Option<PathBuf> [help="save database as table definition and values to SQL file", long],
    read_from: Option<PathBuf> [help="restore database from saved SQL file", long],
    db: PathBuf [help="user authentication database file"]
}

#[tokio::main]
async fn main ()->Result<()> {

    let create_db = !ARGS.db.is_file() && (ARGS.create || ARGS.read_from.is_some());

    if !create_db && !ARGS.db.is_file() {
        return Err( anyhow!("database file not found") )
    }

    let options = SqliteConnectOptions::new()
        .filename(ARGS.db.to_str().unwrap())
        .create_if_missing(create_db);
    let pool = SqlitePool::connect_with(options).await?;

    if create_db {
        if ARGS.read_from.is_some() {
            read_db(&pool).await?;
        } else {
            create_table( &pool).await?;
        }
    } 

    if ARGS.add_user.is_some() {
        add_user( &pool).await?;
    }

    if ARGS.update_user.is_some() {
        update_user( &pool).await?;
    }

    if ARGS.remove_user.is_some() {
        remove_user( &pool).await?;
    }

    if ARGS.find_user.is_some() {
        find_user( &pool).await?;
    }

    if ARGS.list_all {
        list_all( &pool).await?;
    }

    if ARGS.write_to.is_some() {
        write_db( &pool).await?;
    }

    Ok(())
}

async fn create_table (pool: &SqlitePool) -> Result<()> {
    sqlx::query(  r#"
        create table if not exists users
        (
            id integer primary key autoincrement,
            username text not null unique,
            email text not null unique,
            expires integer not null,
            password text not null
        );
    "#)
    .execute(pool).await?;
    Ok(())
}

async fn add_user (pool: &SqlitePool) -> Result<()> {
    if let Some(username) = &ARGS.add_user 
    && let Some(email) = &ARGS.email 
    && let Some(days_valid) = ARGS.days_valid {
        let gen_pw = generate_pw()?;
        let pw = if let Some(pw) = &ARGS.password { pw } else { 
            println!("generated pw for user {}: {}", username, gen_pw);
            &gen_pw 
        };
        let pw_hash = generate_hash(&pw);
        let expires = (Utc::now() + Days::new(days_valid)).timestamp();

        sqlx::query(
            r#"
            INSERT INTO users (username, email, expires, password)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(username)
        .bind(email)
        .bind(expires)
        .bind(pw_hash)
        .execute(pool).await?;

        Ok(())
    } else {
        Err( anyhow!("adding user failed - missing username or email"))
    }
}

fn generate_pw ()->Result<String> {
    let pg = PasswordGenerator {
        length: 8,
        numbers: true,
        lowercase_letters: true,
        uppercase_letters: true,
        symbols: true,
        spaces: false,
        exclude_similar_characters: false,
        strict: true,
    };

    pg.generate_one().map_err(|e| anyhow!(e))
}

async fn update_user (pool: &SqlitePool) -> Result<()> {
    if let Some(username) = &ARGS.remove_user {
        let user: Option<User> = sqlx::query_as(
            r#"
            SELECT id, username, email, expires, password
            FROM users
            WHERE username = ?
            "#,
         )
        .bind(username)
        .fetch_optional(pool)
        .await?;

        if let Some(user) = user {
            if let Some(email) = &ARGS.email { 
                sqlx::query(
                    r#"
                    UPDATE users
                    SET email = ?
                    WHERE username = ?
                    "#,
                )
                .bind( email)
                .bind(username)
                .execute(pool)
                .await?;
            }
            if let Some(days_valid) = ARGS.days_valid {
                let expires = (Utc::now() + Days::new(days_valid)).timestamp();
                sqlx::query(
                    r#"
                    UPDATE users
                    SET expires = ?
                    WHERE username = ?
                    "#,
                )
                .bind(expires)
                .bind(username)
                .execute(pool)
                .await?;
            }
            if let Some(pw) = &ARGS.password {
                let pw_hash = generate_hash(&pw);
                sqlx::query(
                    r#"
                    UPDATE users
                    SET password = ?
                    WHERE username = ?
                    "#,
                )
                .bind(pw_hash)
                .bind(username)
                .execute(pool)
                .await?;
            }

            Ok(())
        } else {
            Err( anyhow!("update user failed - user does not exist"))
        }
    } else {
        Err( anyhow!("update user failed - missing username"))
    }
}

async fn remove_user (pool: &SqlitePool) -> Result<()> {
    if let Some(username) = &ARGS.remove_user {
        sqlx::query(
            r#"
            DELETE FROM users
            WHERE username = ?
            "#,
        )
        .bind(username)
        .execute(pool)
        .await?;

        Ok(())
    } else {
        Err( anyhow!("removing user failed - missing username"))
    }
}

async fn find_user (pool: &SqlitePool) -> Result<()> {
    if let Some(username) = &ARGS.find_user {
        let user: Option<User> = sqlx::query_as(
            r#"
            SELECT id, username, email, expires, password
            FROM users
            WHERE username = ?
            "#,
         )
        .bind(username)
        .fetch_optional(pool)
        .await?;

         if let Some(user) = user {
            println!("{}", user.to_formatted_string());
         } else {
            println!("no matching entry found");
         }

        Ok(())
    } else {
        Err( anyhow!("user lookup failed - missing username"))
    }
}

async fn list_all (pool: &SqlitePool) -> Result<()> {
    let users: Vec<User> = sqlx::query_as(
        r#"
        SELECT id, username, email, expires, password
        FROM users
        ORDER BY username
        "#,
    )
    .fetch_all(pool)
    .await?;

    for user in users {
        println!("{}", user.to_formatted_string());
    }


    Ok(())
}

async fn write_db (pool: &SqlitePool) -> Result<()> {
    if let Some(path) = &ARGS.write_to {
        let mut file = File::create( path)?;

        write!( file, "-- SQLite Database Backup\n");
        write!( file, "-- Generated at: {}\n\n", chrono::Utc::now());

        let schema: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT sql FROM sqlite_master 
            WHERE type='table' AND name='users'
            "#,
        )
        .fetch_all(pool)
        .await?;

        for (create_sql,) in schema {
            write!( file, "{};\n\n", create_sql);
        }

        let rows: Vec<(i64, String, String, i64, String)> = sqlx::query_as(
            r#"
            SELECT id, username, email, expires, password
            FROM users
            ORDER BY id
            "#,
        )
        .fetch_all(pool)
        .await?;

        if !rows.is_empty() {
            write!( file, "-- Insert data\n");
            write!( file, "INSERT INTO users (id, username, email, expires, password) VALUES\n");
            let mut i = rows.len();
            for (id, username, email, expires, password) in rows {
                write!( file,
                    "    ({}, '{}', '{}', '{}', '{}')",
                    id,
                    username.replace("'", "''"),  // Escape single quotes
                    email.replace("'", "''"),
                    expires,
                    password
                );
                i -= 1;
                if i == 0 { write!(file, ";\n"); } else { write!(file, ",\n"); }
            }
        }

        Ok(())
    } else {
        Err( anyhow!("no filename for storing SQL"))
    }
}

async fn read_db (pool: &SqlitePool) -> Result<()> {
    if let Some(path) = &ARGS.read_from {
        let sql_content = read_to_string(path)?;

        for statement in sql_content.split(';') {
            sqlx::query(statement)
                .execute(pool)
                .await?;
        }

        Ok(())
    } else {
        Err( anyhow!("no SQL filename for restoring database"))
    }
}