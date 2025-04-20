use askama::Template;
use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};

#[derive(askama::Template)]
#[template(path = "pages/not_found.html")]
struct NotFoundTemplate;

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
        println!("Server error - {}", &self);
        match &self {
            SameyError::IntConversion(_)
            | SameyError::IntParse(_)
            | SameyError::IO(_)
            | SameyError::Join(_)
            | SameyError::Render(_)
            | SameyError::Database(_)
            | SameyError::Image(_)
            | SameyError::Other(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong!").into_response()
            }
            SameyError::Multipart(_) | SameyError::BadRequest(_) => {
                (StatusCode::BAD_REQUEST, "Invalid request").into_response()
            }
            SameyError::NotFound => (
                StatusCode::NOT_FOUND,
                Html(
                    NotFoundTemplate {}
                        .render()
                        .expect("shouldn't fail to render NotFoundTemplate"),
                ),
            )
                .into_response(),
            SameyError::Authentication(_) => {
                (StatusCode::UNAUTHORIZED, "Not authorized").into_response()
            }
            SameyError::Forbidden => (StatusCode::FORBIDDEN, "Forbidden").into_response(),
        }
    }
}
