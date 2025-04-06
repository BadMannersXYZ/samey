use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[derive(Debug, thiserror::Error)]
pub enum SameyError {
    #[error("Integer conversion error: {0}")]
    IntConversion(#[from] std::num::TryFromIntError),
    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),
    #[error("Task error: {0}")]
    Join(#[from] tokio::task::JoinError),
    #[error("Template render error: {0}")]
    Render(#[from] askama::Error),
    #[error("Database error: {0}")]
    Database(#[from] sea_orm::error::DbErr),
    #[error("File streaming error: {0}")]
    Multipart(#[from] axum::extract::multipart::MultipartError),
    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("Internal error: {0}")]
    Other(String),
    #[error("Not found")]
    NotFound,
}

impl IntoResponse for SameyError {
    fn into_response(self) -> Response {
        println!("Server error - {}", &self);
        match &self {
            SameyError::IntConversion(_)
            | SameyError::IO(_)
            | SameyError::Join(_)
            | SameyError::Render(_)
            | SameyError::Database(_)
            | SameyError::Image(_)
            | SameyError::Other(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong!").into_response()
            }
            SameyError::Multipart(_) => {
                (StatusCode::BAD_REQUEST, "Invalid request").into_response()
            }
            SameyError::NotFound => (StatusCode::NOT_FOUND, "Resource not found").into_response(),
        }
    }
}
