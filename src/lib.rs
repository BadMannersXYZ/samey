pub(crate) mod auth;
pub(crate) mod entities;
pub(crate) mod error;
pub(crate) mod query;
pub(crate) mod rating;
pub(crate) mod views;

use std::sync::Arc;

use auth::SessionStorage;
use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{delete, get, post, put},
};
use axum_login::AuthManagerLayerBuilder;
use password_auth::generate_hash;
use sea_orm::{ActiveValue::Set, DatabaseConnection, EntityTrait};
use tokio::fs;
use tower_http::services::ServeDir;
use tower_sessions::SessionManagerLayer;

use crate::auth::Backend;
use crate::entities::{prelude::SameyUser, samey_user};
pub use crate::error::SameyError;
use crate::views::{
    add_post_source, delete_post, edit_post_details, get_full_media, get_media, index, login,
    logout, post_details, posts, posts_page, remove_field, search_tags, select_tag,
    submit_post_details, upload, view_post,
};

pub(crate) const NEGATIVE_PREFIX: &str = "-";
pub(crate) const RATING_PREFIX: &str = "rating:";

#[derive(Clone)]
pub(crate) struct AppState {
    files_dir: Arc<String>,
    db: DatabaseConnection,
}

pub async fn create_user(
    db: DatabaseConnection,
    username: String,
    password: String,
    is_admin: bool,
) -> Result<(), SameyError> {
    SameyUser::insert(samey_user::ActiveModel {
        username: Set(username),
        password: Set(generate_hash(password)),
        is_admin: Set(is_admin),
        ..Default::default()
    })
    .exec(&db)
    .await?;
    Ok(())
}

pub async fn get_router(db: DatabaseConnection, files_dir: &str) -> Result<Router, SameyError> {
    let state = AppState {
        files_dir: Arc::new(files_dir.into()),
        db: db.clone(),
    };
    fs::create_dir_all(files_dir).await?;

    let session_store = SessionStorage::new(db.clone());
    let session_layer = SessionManagerLayer::new(session_store);
    let auth_layer = AuthManagerLayerBuilder::new(Backend::new(db), session_layer).build();

    Ok(Router::new()
        .route("/login", post(login))
        .route("/logout", get(logout))
        .route(
            "/upload",
            post(upload).layer(DefaultBodyLimit::max(100_000_000)),
        )
        .route("/search_tags", post(search_tags))
        .route("/select_tag/{new_tag}", post(select_tag))
        .route("/posts", get(posts))
        .route("/posts/{page}", get(posts_page))
        .route("/view/{post_id}", get(view_post))
        .route("/post/{post_id}", delete(delete_post))
        .route("/post_details/{post_id}/edit", get(edit_post_details))
        .route("/post_details/{post_id}", get(post_details))
        .route("/post_details/{post_id}", put(submit_post_details))
        .route("/post_source", post(add_post_source))
        .route("/remove", delete(remove_field))
        .route("/media/{post_id}/full", get(get_full_media))
        .route("/media/{post_id}", get(get_media))
        .route("/", get(index))
        .with_state(state)
        .nest_service("/files", ServeDir::new(files_dir))
        .layer(auth_layer))
}
