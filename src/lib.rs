//! Sam's small image board.

pub(crate) mod auth;
pub(crate) mod config;
pub(crate) mod entities;
pub(crate) mod error;
pub(crate) mod query;
pub(crate) mod tags;
pub(crate) mod video;
pub(crate) mod views;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{StatusCode, header::CONTENT_TYPE},
    response::IntoResponse,
    routing::{delete, get, post, put},
};
use axum_extra::routing::RouterExt;
use axum_login::AuthManagerLayerBuilder;
use password_auth::generate_hash;
use sea_orm::{ActiveValue::Set, DatabaseConnection, EntityTrait};
use tokio::{fs, sync::RwLock};
use tower_http::services::ServeDir;
use tower_sessions::SessionManagerLayer;

use crate::auth::{Backend, SessionStorage};
use crate::config::AppConfig;
use crate::entities::{prelude::SameyUser, samey_user};
pub use crate::error::SameyError;
use crate::views::*;

#[derive(rust_embed::Embed)]
#[folder = "static/"]
struct Asset;

fn assets_router() -> Router {
    Router::new().route(
        "/{*file}",
        get(|uri: axum::http::Uri| async move {
            let path = uri.path().trim_start_matches('/');
            match Asset::get(path) {
                Some(content) => {
                    let mime = mime_guess::MimeGuess::from_path(path).first_or_octet_stream();
                    ([(CONTENT_TYPE, mime.as_ref())], content.data).into_response()
                }
                None => StatusCode::NOT_FOUND.into_response(),
            }
        }),
    )
}

#[derive(Clone)]
pub(crate) struct AppState {
    files_dir: Arc<PathBuf>,
    db: DatabaseConnection,
    app_config: Arc<RwLock<AppConfig>>,
}

/// Helper function to create a single user.
///
/// You can specify if they must be an admin user via the `is_admin` flag.
///
/// ```
/// use samey::create_user;
///
/// # async fn _main() {
/// let db = sea_orm::Database::connect("sqlite:db.sqlite3?mode=rwc").await.unwrap();
/// create_user(db, "admin", "secretPassword", true).await.expect("Unable to add admin user");
/// # }
/// ```
pub async fn create_user(
    db: DatabaseConnection,
    username: &str,
    password: &str,
    is_admin: bool,
) -> Result<(), SameyError> {
    SameyUser::insert(samey_user::ActiveModel {
        username: Set(username.into()),
        password: Set(generate_hash(password)),
        is_admin: Set(is_admin),
        ..Default::default()
    })
    .exec(&db)
    .await?;
    Ok(())
}

/// Creates an Axum router for a Samey application.
///
/// ```
/// use samey::get_router;
///
/// # async fn _main() {
/// let db = sea_orm::Database::connect("sqlite:db.sqlite3?mode=rwc").await.unwrap();
/// let app = get_router(db, "files").await.unwrap();
/// let listener = tokio::net::TcpListener::bind(("0.0.0.0", 3000)).await.unwrap();
/// axum::serve(listener, app).await.unwrap();
/// # }
/// ```
pub async fn get_router(
    db: DatabaseConnection,
    files_dir: impl AsRef<Path>,
) -> Result<Router, SameyError> {
    let state = AppState {
        files_dir: Arc::new(files_dir.as_ref().to_owned()),
        db: db.clone(),
        app_config: Arc::new(RwLock::new(AppConfig::new(&db).await?)),
    };
    fs::create_dir_all(files_dir.as_ref()).await?;

    let session_store = SessionStorage::new(db.clone());
    let session_layer = SessionManagerLayer::new(session_store).with_expiry(
        tower_sessions::Expiry::OnInactivity(time::Duration::weeks(1)),
    );
    let auth_layer = AuthManagerLayerBuilder::new(Backend::new(db), session_layer).build();

    Ok(Router::new()
        // Auth routes
        .route_with_tsr("/login", get(login_page).post(login))
        .route_with_tsr("/logout", get(logout))
        // Tags routes
        .route_with_tsr("/search_tags", post(search_tags))
        .route_with_tsr("/select_tag", post(select_tag))
        // Post routes
        .route_with_tsr(
            "/upload",
            get(upload_page)
                .post(upload)
                .layer(DefaultBodyLimit::max(100_000_000)),
        )
        .route_with_tsr("/post/{post_id}", get(view_post_page).delete(delete_post))
        .route_with_tsr("/post_details/{post_id}/edit", get(edit_post_details))
        .route_with_tsr(
            "/post_details/{post_id}",
            get(post_details).put(submit_post_details),
        )
        .route_with_tsr("/post_source", post(add_post_source))
        // Pool routes
        .route_with_tsr("/create_pool", get(create_pool_page))
        .route_with_tsr("/pools", get(get_pools))
        .route_with_tsr("/pools/{page}", get(get_pools_page))
        .route_with_tsr("/pool", post(create_pool))
        .route_with_tsr("/pool/{pool_id}", get(view_pool).delete(delete_pool))
        .route_with_tsr("/pool/{pool_id}/name", put(change_pool_name))
        .route_with_tsr("/pool/{pool_id}/public", put(change_pool_visibility))
        .route_with_tsr("/pool/{pool_id}/post", post(add_post_to_pool))
        .route_with_tsr("/pool/{pool_id}/sort", put(sort_pool))
        .route_with_tsr("/pool_post/{pool_post_id}", delete(remove_pool_post))
        // Bulk edit tag routes
        .route_with_tsr("/bulk_edit_tag", get(bulk_edit_tag).post(edit_tag))
        // Settings routes
        .route_with_tsr("/settings", get(settings).post(update_settings))
        // Search routes
        .route_with_tsr("/posts", get(posts))
        .route_with_tsr("/posts/{page}", get(posts_page))
        // Other routes
        .route_with_tsr("/remove", delete(remove_field))
        .route("/posts.xml", get(rss_page))
        .route("/", get(index))
        .with_state(state)
        .nest_service("/files", ServeDir::new(files_dir))
        .nest("/static", assets_router())
        .layer(auth_layer))
}
