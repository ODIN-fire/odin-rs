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

use axum::{
    extract::Query,
    http::StatusCode,
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
    Form, Router,
};
//use axum_messages::{Message, Messages};
use serde::{Serialize,Deserialize};
use axum_login::{AuthUser, AuthnBackend, UserId};
use password_auth::verify_password;
use sqlx::{FromRow, SqlitePool};
use tokio::task;
use chrono::{DateTime,Utc,SecondsFormat};

use crate::errors::OdinServerError;

/* #region user and session model *************************************************************************/

/*
-- Create users table.
create table if not exists users
(
    id integer primary key not null,
    username text not null unique,
    password text not null
);

-- Insert user.
insert into users (id, username, password)
values
   (1, 'gonzo', '$argon2id$v=19$m=19456,t=2,p=1$VE0e3g7DalWHgDwou3nuRA$uC6TER156UQpk0lNQ5+jHM0l5poVjPA1he/Tyn9J4Zw');
 */

#[derive(Clone, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub expires: i64, // epoch seconds
    password: String,
}

impl User {
    pub fn to_formatted_string (&self)->String {
        let exp = self.expiration_datetime();
        format!("{} : {}, {}, {}, {}", 
            self.username, self.id, self.email, 
            self.expiration_datetime().to_rfc3339_opts(SecondsFormat::Secs, true), 
            self.password)
    }

    pub fn expiration_datetime (&self)->DateTime<Utc> {
        if let Some(dt) = DateTime::from_timestamp( self.expires, 0) {
            dt
        } else {
            DateTime::<Utc>::MIN_UTC
        }
    }
}

/// make sure we don't show passwords
impl std::fmt::Debug for User {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("User")
            .field("id", &self.id)
            .field("username", &self.username)
            .field("email", &self.username)
            .field("expires", &DateTime::from_timestamp( self.expires, 0).map_or_else( || "<invalid>".to_string(), |dt| dt.to_rfc2822(),))
            .finish()
    }
}

/// the axum-login interface for User
impl AuthUser for User {
    type Id = i64;

    fn id(&self) -> Self::Id { self.id }

    /// NOTE - changing passwords invalidates open sessions
    fn session_auth_hash(&self) -> &[u8] { self.password.as_bytes() }
}

/* #endregion user and session model */

/* #region backend ****************************************************************************************/

/// store for user/session data
#[derive(Debug, Clone)]
pub struct Backend {
    db: SqlitePool,
}

impl Backend {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

/// the axum-login interface for Backend
impl AuthnBackend for Backend {
    type User = User;
    type Credentials = Credentials;
    type Error = OdinServerError;

    async fn authenticate (&self,creds: Self::Credentials) -> Result<Option<Self::User>, Self::Error> {
        let user: Option<Self::User> = sqlx::query_as("select * from users where username = ? ")
            .bind(creds.username)
            .fetch_optional(&self.db)
            .await?;

        // this is slow so it has to be spawned
        task::spawn_blocking(|| {
            // pw-based - directly compare the form input with the stored hash
            Ok(user.filter(|user| verify_password(creds.password, &user.password).is_ok()))
        })
        .await?
    }

    async fn get_user (&self, user_id: &UserId<Self>) -> Result<Option<Self::User>, Self::Error> {
        let user = sqlx::query_as("select * from users where id = ?")
            .bind(user_id)
            .fetch_optional(&self.db)
            .await?;

        Ok(user)
    }
}

pub type AuthSession = axum_login::AuthSession<Backend>;

/* #endregion backend */

/*  #region auth route handlers ***************************************************************************/

/// login POST data 
#[derive(Debug, Clone, Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
    pub next: Option<String>,
}

/// login GET query param
#[derive(Debug, Deserialize)]
pub struct NextUrl {
    next: Option<String>,
}

/// the routes related to user authentication
pub fn router() -> Router<()> {
    Router::new()
        .route("/login", post(self::post::login))
        .route("/login", get(self::get::login))
        .route("/logout", get(self::get::logout))
}

mod post {
    use super::*;

    /// the login dialog response
    pub async fn login (mut auth_session: AuthSession, Form(creds): Form<Credentials>) -> impl IntoResponse {
        let user = match auth_session.authenticate(creds.clone()).await {
            Ok(Some(user)) => user,
            Ok(None) => {
                //messages.error("Invalid credentials");

                let mut login_url = "/login".to_string();
                if let Some(next) = creds.next {
                    login_url = format!("{login_url}?next={next}");
                };

                return Redirect::to(&login_url).into_response();
            }
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };

        if auth_session.login(&user).await.is_err() {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }

        //messages.success(format!("Successfully logged in as {}", user.username));

        if let Some(ref next) = creds.next {
            Redirect::to(next)
        } else {
            Redirect::to("/")
        }
        .into_response()
    }
}

mod get {
    use super::*;

    /// the login dialog
    pub async fn login (Query(NextUrl { next }): Query<NextUrl>) -> impl IntoResponse {
        (StatusCode::OK, "login TBD")
    }

    /// the logout dialog
    pub async fn logout(mut auth_session: AuthSession) -> impl IntoResponse {
        match auth_session.logout().await {
            Ok(_) => Redirect::to("/login").into_response(),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}

/* #endregion auth route handlers */