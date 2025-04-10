use std::fmt::Debug;

use axum_login::{AuthUser, AuthnBackend, UserId};
use migration::Expr;
use password_auth::verify_password;
use sea_orm::{ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use serde::Deserialize;
use time::OffsetDateTime;
use tower_sessions::{ExpiredDeletion, SessionStore, session::Record, session_store};

use crate::{
    SameyError,
    entities::{
        prelude::{SameySession, SameyUser},
        samey_session, samey_user,
    },
};

#[derive(Debug, Clone)]
pub(crate) struct User {
    pub(crate) id: i32,
    pub(crate) username: String,
    pub(crate) is_admin: bool,
}

impl AuthUser for User {
    type Id = i32;

    fn id(&self) -> Self::Id {
        self.id
    }

    fn session_auth_hash(&self) -> &[u8] {
        self.username.as_bytes()
    }
}

#[derive(Clone, Deserialize)]
pub(crate) struct Credentials {
    pub(crate) username: String,
    pub(crate) password: String,
}

impl Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &"[redacted]")
            .finish()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Backend {
    db: DatabaseConnection,
}

impl Backend {
    pub(crate) fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }
}

#[async_trait::async_trait]
impl AuthnBackend for Backend {
    type User = User;
    type Credentials = Credentials;
    type Error = SameyError;

    async fn authenticate(
        &self,
        credentials: Self::Credentials,
    ) -> Result<Option<Self::User>, Self::Error> {
        let user = SameyUser::find()
            .filter(samey_user::Column::Username.eq(credentials.username))
            .one(&self.db)
            .await?;

        Ok(user.and_then(|user| {
            verify_password(credentials.password, &user.password)
                .ok()
                .map(|_| User {
                    id: user.id,
                    username: user.username,
                    is_admin: user.is_admin,
                })
        }))
    }

    async fn get_user(&self, user_id: &UserId<Self>) -> Result<Option<Self::User>, Self::Error> {
        let user = SameyUser::find_by_id(*user_id).one(&self.db).await?;

        Ok(user.map(|user| User {
            id: user.id,
            username: user.username,
            is_admin: user.is_admin,
        }))
    }
}

pub(crate) type AuthSession = axum_login::AuthSession<Backend>;

#[derive(Debug, Clone)]
pub(crate) struct SessionStorage {
    db: DatabaseConnection,
}

impl SessionStorage {
    pub(crate) fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }
}

#[async_trait::async_trait]
impl SessionStore for SessionStorage {
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        SameySession::insert(samey_session::ActiveModel {
            session_id: Set(record.id.to_string()),
            data: Set(sea_orm::JsonValue::Object(
                record
                    .data
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            )),
            expiry_date: Set(record.expiry_date.unix_timestamp()),
            ..Default::default()
        })
        .exec(&self.db)
        .await
        .map_err(|_| session_store::Error::Backend("Failed to create a new session".into()))?;
        Ok(())
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        let session = SameySession::find()
            .filter(samey_session::Column::SessionId.eq(record.id.to_string()))
            .one(&self.db)
            .await
            .map_err(|_| session_store::Error::Backend("Failed to find session".into()))?
            .ok_or(session_store::Error::Backend(
                "No corresponding session found".into(),
            ))?;
        SameySession::update(samey_session::ActiveModel {
            id: Set(session.id),
            data: Set(sea_orm::JsonValue::Object(
                record
                    .data
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            )),
            expiry_date: Set(record.expiry_date.unix_timestamp()),
            ..Default::default()
        })
        .exec(&self.db)
        .await
        .map_err(|_| session_store::Error::Backend("Failed to update session".into()))?;
        Ok(())
    }

    async fn load(
        &self,
        session_id: &tower_sessions::session::Id,
    ) -> session_store::Result<Option<Record>> {
        let session = SameySession::find()
            .filter(samey_session::Column::SessionId.eq(session_id.to_string()))
            .one(&self.db)
            .await
            .map_err(|_| session_store::Error::Backend("Failed to retrieve session".into()))?;

        let record = match session {
            Some(session) => Record {
                id: session.session_id.parse().map_err(|_| {
                    session_store::Error::Backend("Failed to parse session ID".into())
                })?,
                data: session
                    .data
                    .as_object()
                    .ok_or(session_store::Error::Backend(
                        "Failed to parse session data".into(),
                    ))?
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
                expiry_date: OffsetDateTime::from_unix_timestamp(session.expiry_date).map_err(
                    |_| session_store::Error::Backend("Invalid timestamp for expiry date".into()),
                )?,
            },
            None => return Ok(None),
        };
        if record.expiry_date > OffsetDateTime::now_utc() {
            Ok(Some(record))
        } else {
            Ok(None)
        }
    }

    async fn delete(&self, session_id: &tower_sessions::session::Id) -> session_store::Result<()> {
        SameySession::delete_many()
            .filter(samey_session::Column::SessionId.eq(session_id.to_string()))
            .exec(&self.db)
            .await
            .map_err(|_| session_store::Error::Backend("Failed to delete session".into()))?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl ExpiredDeletion for SessionStorage {
    async fn delete_expired(&self) -> session_store::Result<()> {
        SameySession::delete_many()
            .filter(Expr::cust(
                "DATETIME(\"samey_session\".\"expiry_date\", 'unixepoch') < DATETIME('now')",
            ))
            .exec(&self.db)
            .await
            .map_err(|_| {
                session_store::Error::Backend("Failed to delete expired sessions".into())
            })?;
        Ok(())
    }
}
