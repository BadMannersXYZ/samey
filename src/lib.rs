pub(crate) mod auth;
pub(crate) mod config;
pub(crate) mod entities;
pub(crate) mod error;
pub(crate) mod query;
pub(crate) mod rating;
pub(crate) mod video;
pub(crate) mod views;

use std::sync::Arc;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{delete, get, post, put},
};
use axum_login::AuthManagerLayerBuilder;
use entities::{prelude::SameyConfig, samey_config};
use password_auth::generate_hash;
use sea_orm::{ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use tokio::{fs, sync::RwLock};
use tower_http::services::ServeDir;
use tower_sessions::SessionManagerLayer;

use crate::auth::{Backend, SessionStorage};
use crate::config::APPLICATION_NAME_KEY;
use crate::entities::{prelude::SameyUser, samey_user};
pub use crate::error::SameyError;
use crate::views::{
    add_post_source, add_post_to_pool, change_pool_visibility, create_pool, delete_post,
    edit_post_details, get_full_media, get_media, get_pools, get_pools_page, index, login, logout,
    post_details, posts, posts_page, remove_field, remove_pool_post, search_tags, select_tag,
    settings, sort_pool, submit_post_details, update_settings, upload, view_pool, view_post,
};

pub(crate) const NEGATIVE_PREFIX: &str = "-";
pub(crate) const RATING_PREFIX: &str = "rating:";

#[derive(Clone)]
pub(crate) struct AppState {
    files_dir: Arc<String>,
    db: DatabaseConnection,
    application_name: Arc<RwLock<String>>,
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
    let application_name = match SameyConfig::find()
        .filter(samey_config::Column::Key.eq(APPLICATION_NAME_KEY))
        .one(&db)
        .await?
    {
        Some(row) => row.data.as_str().unwrap_or("Samey").to_owned(),
        None => "Samey".to_owned(),
    };
    let state = AppState {
        files_dir: Arc::new(files_dir.into()),
        db: db.clone(),
        application_name: Arc::new(RwLock::new(application_name)),
    };
    fs::create_dir_all(files_dir).await?;

    let session_store = SessionStorage::new(db.clone());
    let session_layer = SessionManagerLayer::new(session_store);
    let auth_layer = AuthManagerLayerBuilder::new(Backend::new(db), session_layer).build();

    Ok(Router::new()
        // Auth routes
        .route("/login", post(login))
        .route("/logout", get(logout))
        // Tags routes
        .route("/search_tags", post(search_tags))
        .route("/select_tag", post(select_tag))
        // Post routes
        .route(
            "/upload",
            post(upload).layer(DefaultBodyLimit::max(100_000_000)),
        )
        .route("/post/{post_id}", get(view_post).delete(delete_post))
        .route("/post_details/{post_id}/edit", get(edit_post_details))
        .route(
            "/post_details/{post_id}",
            get(post_details).put(submit_post_details),
        )
        .route("/post_source", post(add_post_source))
        .route("/media/{post_id}/full", get(get_full_media))
        .route("/media/{post_id}", get(get_media))
        // Pool routes
        .route("/pools", get(get_pools))
        .route("/pools/{page}", get(get_pools_page))
        .route("/pool", post(create_pool))
        .route("/pool/{pool_id}", get(view_pool))
        .route("/pool/{pool_id}/public", put(change_pool_visibility))
        .route("/pool/{pool_id}/post", post(add_post_to_pool))
        .route("/pool/{pool_id}/sort", put(sort_pool))
        .route("/pool_post/{pool_post_id}", delete(remove_pool_post))
        // Settings routes
        .route("/settings", get(settings).put(update_settings))
        // Search routes
        .route("/posts", get(posts))
        .route("/posts/{page}", get(posts_page))
        // Other routes
        .route("/remove", delete(remove_field))
        .route("/", get(index))
        .with_state(state)
        .nest_service("/files", ServeDir::new(files_dir))
        .layer(auth_layer))
}
