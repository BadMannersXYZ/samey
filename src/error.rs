use askama::Template;
use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};

#[derive(askama::Template)]
#[template(path = "pages/bad_request.html")]
struct BadRequestTemplate<'a> {
    error: &'a str,
}

#[derive(askama::Template)]
#[template(path = "pages/unauthorized.html")]
struct UnauthorizedTemplate;

#[derive(askama::Template)]
#[template(path = "pages/forbidden.html")]
struct ForbiddenTemplate;

#[derive(askama::Template)]
#[template(path = "pages/not_found.html")]
struct NotFoundTemplate;

#[derive(askama::Template)]
#[template(path = "pages/internal_server_error.html")]
struct InternalServerErrorTemplate;

/// Errors from Samey.
#[derive(Debug, thiserror::Error)]
pub enum SameyError {
    /// Integer conversion error.
    #[error("Integer conversion error: {0}")]
    IntConversion(#[from] std::num::TryFromIntError),
    /// Integer parsing error.
    #[error("Integer parsing error: {0}")]
    IntParse(#[from] std::num::ParseIntError),
    /// IO error.
    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),
    /// Task error.
    #[error("Task error: {0}")]
    Join(#[from] tokio::task::JoinError),
    /// Template render error.
    #[error("Template render error: {0}")]
    Render(#[from] askama::Error),
    /// Database error.
    #[error("Database error: {0}")]
    Database(#[from] sea_orm::error::DbErr),
    /// File streaming error.
    #[error("File streaming error: {0}")]
    Multipart(#[from] axum::extract::multipart::MultipartError),
    /// Image error.
    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),
    /// Authentication error.
    #[error("Authentication error: {0}")]
    Authentication(String),
    /// Not found.
    #[error("Not found")]
    NotFound,
    /// Not allowed.
    #[error("Not allowed")]
    Forbidden,
    /// Bad request.
    #[error("Bad request: {0}")]
    BadRequest(String),
    /// Custom internal error.
    #[error("Internal error: {0}")]
    Other(String),
}

impl IntoResponse for SameyError {
    fn into_response(self) -> Response {
        match &self {
            SameyError::IntConversion(_)
            | SameyError::IntParse(_)
            | SameyError::IO(_)
            | SameyError::Join(_)
            | SameyError::Render(_)
            | SameyError::Database(_)
            | SameyError::Image(_)
            | SameyError::Other(_) => {
                println!("Internal server error - {:?}", &self);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Html(
                        InternalServerErrorTemplate {}
                            .render()
                            .expect("shouldn't fail to render InternalServerErrorTemplate"),
                    ),
                )
                    .into_response()
            }
            SameyError::Multipart(error) => (
                StatusCode::BAD_REQUEST,
                Html(
                    BadRequestTemplate {
                        error: &error.body_text(),
                    }
                    .render()
                    .expect("shouldn't fail to render BadRequestTemplate"),
                ),
            )
                .into_response(),
            SameyError::BadRequest(error) => (
                StatusCode::BAD_REQUEST,
                Html(
                    BadRequestTemplate { error }
                        .render()
                        .expect("shouldn't fail to render BadRequestTemplate"),
                ),
            )
                .into_response(),
            SameyError::NotFound => (
                StatusCode::NOT_FOUND,
                Html(
                    NotFoundTemplate {}
                        .render()
                        .expect("shouldn't fail to render NotFoundTemplate"),
                ),
            )
                .into_response(),
            SameyError::Authentication(_) => (
                StatusCode::UNAUTHORIZED,
                Html(
                    UnauthorizedTemplate {}
                        .render()
                        .expect("shouldn't fail to render UnauthorizedTemplate"),
                ),
            )
                .into_response(),
            SameyError::Forbidden => (
                StatusCode::FORBIDDEN,
                Html(
                    ForbiddenTemplate {}
                        .render()
                        .expect("shouldn't fail to render ForbiddenTemplate"),
                ),
            )
                .into_response(),
        }
    }
}
