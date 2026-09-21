//! The end of the log, for a failure read on the page rather than over ssh.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use super::{Shared, wants_json};

const LINES: usize = 200;
const MOST: usize = 2000;

#[derive(Debug, Default, Deserialize)]
pub struct Asked {
    pub lines: Option<usize>,
}

#[derive(Serialize)]
struct Tail {
    path: String,
    lines: Vec<String>,
}

pub async fn tail(
    State(control): State<Shared>,
    headers: HeaderMap,
    Query(asked): Query<Asked>,
) -> Response {
    let lines = asked.lines.unwrap_or(LINES).clamp(1, MOST);
    let path = crate::log::path(&control.operation().config.config.state_dir());
    let text = match tokio::task::spawn_blocking({
        let path = path.clone();
        move || crate::log::tail(&path, lines)
    })
    .await
    {
        Ok(Ok(text)) => text,
        Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return (
                StatusCode::NOT_FOUND,
                "no log file: this server writes to the terminal it was started from\n",
            )
                .into_response();
        }
        Ok(Err(error)) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("the log could not be read: {error}\n"),
            )
                .into_response();
        }
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("the log could not be read: {error}\n"),
            )
                .into_response();
        }
    };
    if wants_json(&headers) {
        let tail = Tail {
            path: path.display().to_string(),
            lines: text.lines().map(str::to_owned).collect(),
        };
        return match serde_json::to_string(&tail) {
            Ok(body) => (
                [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
                body,
            )
                .into_response(),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
    }
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response()
}
