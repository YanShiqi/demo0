use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use tracing::error;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("请求内容无效：{0}")]
    BadRequest(String),
    #[error("请先登录")]
    Unauthorized,
    #[error("没有执行此操作的权限")]
    Forbidden,
    #[error("{0}")]
    TooManyRequests(String),
    #[error("内容不存在")]
    NotFound,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Template(#[from] askama::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, message.clone()),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, self.to_string()),
            Self::Forbidden => (StatusCode::FORBIDDEN, self.to_string()),
            Self::TooManyRequests(message) => (StatusCode::TOO_MANY_REQUESTS, message.clone()),
            Self::NotFound => (StatusCode::NOT_FOUND, self.to_string()),
            Self::Database(_) | Self::Template(_) | Self::Io(_) | Self::Internal(_) => {
                error!(error = %self, "请求处理失败");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "服务器暂时无法处理请求".to_owned(),
                )
            }
        };

        (status, message).into_response()
    }
}
