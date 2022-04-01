use axum::body::{self, BoxBody};
use axum::extract::{Multipart, Path, TypedHeader};
use axum::headers::{Authorization, authorization::Bearer};
use axum::http::{header::CONTENT_TYPE, HeaderValue, StatusCode};
use axum::response::{Json, Response};
use axum::routing::{delete, get, post};
use axum::Router;

use tower_http::trace::TraceLayer;

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use dotenv::dotenv;

use futures::stream::StreamExt;
use serde_json::{json, Value};

use mime_guess::from_path;

use std::io::ErrorKind::{AlreadyExists, NotFound};
use std::net::SocketAddr;

use tokio::fs;

async fn index() -> Json<Value> {
    Json(json!({
        "message": "Hello, World!"
    }))
}

async fn upload_file(
    TypedHeader(authorization): TypedHeader<Authorization<Bearer>>,
    mut multipart: Multipart,
) -> Result<Json<Value>, StatusCode> {
    if authorization.token() != std::env::var("AUTH_TOKEN").unwrap_or_else(|_| "aaa".to_string()).as_str() {
        return Err(StatusCode::UNAUTHORIZED);
    }

    if let Ok(Some(mut field)) = multipart.next_field().await {
        if field.file_name().is_some() {
            let filename = field.file_name().unwrap_or_else(|| unreachable!()).to_string();
            let path = format!("/home/services/cdn/uploads/{}", filename);

            let mut buffer: Vec<u8> = Vec::new();
            let mut file_size: u64 = 0;

            while let Some(chunk) = field.next().await {
                let data = chunk.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

                file_size += data.len() as u64;

                if file_size > 10 * 1024 * 1024 {
                    return Err(StatusCode::PAYLOAD_TOO_LARGE);
                }

                buffer.extend_from_slice(&data);
            }

            fs::write(path, &buffer[..])
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            return Ok(Json(json!({
                "message": "File uploaded"
            })));
        } else {
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    Err(StatusCode::BAD_REQUEST)
}

async fn get_file(Path(filename): Path<String>) -> Result<Response<BoxBody>, StatusCode> {
    let path = format!("/home/services/cdn/uploads/{}", filename);

    let file = fs::read(path)
        .await
        .map_err(|e| match e.kind() {
            NotFound => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let resp = Response::builder()
        .status(StatusCode::OK)
        .header(
            CONTENT_TYPE,
            HeaderValue::from_str(
                from_path(filename)
                    .first_or_octet_stream()
                    .to_string()
                    .as_str(),
            ).unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
        )
        .body(body::boxed(body::Full::from(file)))
        .unwrap_or_else(|e| unreachable!("{e:?}"));

    Ok(resp)
}

async fn delete_file(Path(filename): Path<String>) -> Result<Json<Value>, StatusCode> {
    let path = format!("/home/services/cdn/uploads/{}", filename);

    fs::remove_file(path)
        .await
        .map_err(|e| match e.kind() {
            NotFound => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(json!({
        "message": "File deleted"
    })))
}

#[tokio::main]
async fn main() {
    dotenv().ok();

    tracing_subscriber::registry()
    .with(tracing_subscriber::EnvFilter::new(
        std::env::var("RUST_LOG")
            .unwrap_or_else(|_| "tower_http=debug".into()),
    ))
    .with(tracing_subscriber::fmt::layer())
    .init();


    let _ =
        fs::create_dir_all("/home/services/cdn/uploads".to_string()).await.map_err(|e| match e.kind() {
            AlreadyExists => (),
            _ => panic!("{e:?}"),
        });

    let router = Router::new()
        .route("/", get(index))
        .route("/upload", post(upload_file))
        .route("/uploads/:filename", get(get_file))
        .route("/uploads/:filename", delete(delete_file))
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([0, 0, 0, 0], 8083));

    let server = axum::Server::bind(&addr)
        .serve(router.into_make_service())
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c()
                .await
                .expect("Failed to wait for CTRL+C");
        });

    server.await.expect("Failed to start server");
}
