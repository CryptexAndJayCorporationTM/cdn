use axum::body::{self, BoxBody};
use axum::extract::{Multipart, Path, TypedHeader};
use axum::headers::{Authorization, ContentType};
use axum::http::{header::CONTENT_TYPE, HeaderValue, StatusCode};
use axum::response::Json;
use axum::routing::{delete, get, post};
use axum::Router;

use serde_json::{json, Value};

use mine_guess::from_path;

use std::io::ErrorKind::AlreadyExists;
use std::net::SocketAddr;

use tokio::fs;

async fn index() -> Json<Value> {
    json!({
        "message": "Hello, World!"
    })
}

async fn upload_file(
    TypedHeader(authorization): TypedHeader<Authorization>,
    mut multipart: Multipart,
) -> Result<Json<Value>, StatusCode> {
    if authorization != "aaa" {
        Err(StatusCode::Unauthorized)
    }

    if let Ok(Some(mut field)) = multipart.next_field().await {
        if let Some(filename) = field.filename {
            let path = format!("/home/services/cdn/uploads/{}", filename);

            let mut file = fs::File::create(path)
                .await
                .map_err(|_| StatusCode::InternalServerError)?;

            let mut buffer: Vec<u8> = Vec::new();
            let mut file_size: u64 = 0;

            while let Some(chunk) = field.next().await {
                let data = chunk.map_err(|_| StatusCode::InternalServerError)?;

                file_size += data.len() as u64;

                if file_size > 10 * 1024 * 1024 {
                    return Err(StatusCode::RequestEntityTooLarge);
                }

                buffer.extend_from_slice(&data);
            }

            fs::write(filename, &buffer[..])
                .await
                .map_err(|_| StatusCode::InternalServerError)?;

            Ok(json!({
                "message": "File uploaded"
            }))
        } else {
            Err(StatusCode::BadRequest)
        }
    }

    Err(StatusCode::BadRequest)
}

async fn get_file(Path(filename): String) -> Result<Response<BoxBody>, StatusCode> {
    let path = format!("/home/services/cdn/uploads/{}", filename);

    let file = fs::read(path)
        .await
        .map_err(|_| StatusCode::InternalServerError)?;

    let resp = Response::builder()
        .status(StatusCode::OK)
        .header(
            CONTENT_TYPE,
            HeaderValue::from_str(
                from_path(filename)
                    .first_or_octet_stream()
                    .to_string()
                    .as_str(),
            ),
        )
        .body(body::boxed(BoxBody::from(file)))
        .unwrap_or_else(|e| unreachable!("{e:?}"));

    Ok(resp)
}

async fn delete_file(Path(filename): String) -> Result<Json<Value>, StatusCode> {
    let path = format!("/home/services/cdn/uploads/{}", filename);

    fs::remove_file(path)
        .await
        .map_err(|_| StatusCode::InternalServerError)?;

    Ok(json!({
        "message": "File deleted"
    }))
}

#[tokio::main]
async fn main() {
    let _ =
        fs::create_dir_all("/home/services/cdn/uploads".to_string()).map_err(|e| match e.kind() {
            AlreadyExists => (),
            _ => panic!("{e:?}"),
        });

    let router = Router::new()
        .route("/", get(index))
        .route("/upload", post(upload_file))
        .route("/uploads/:filename", get(get_file))
        .route("/uploads/:filename", delete(delete_file));

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
