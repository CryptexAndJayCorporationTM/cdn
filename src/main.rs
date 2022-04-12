use axum::body::{self, BoxBody};
use axum::extract::{Multipart, Path, Query, TypedHeader};
use axum::headers::{authorization::Bearer, Authorization};
use axum::http::{header::CONTENT_TYPE, HeaderValue, StatusCode};
use axum::response::{Json, Response};
use axum::routing::{get, post};
use axum::Router;

use tower_http::trace::TraceLayer;

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use dotenv::dotenv;

use futures::stream::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};

use mime_guess::from_path;

use std::io::ErrorKind::{AlreadyExists, NotFound};
use std::net::SocketAddr;
use std::path::Path as StdPath;

use tokio::fs;

#[derive(Deserialize)]
struct UploadFileQuery {
    directory: Option<String>,
    #[serde(default = "default_false")]
    safe: bool,
}

fn default_false() -> bool {
    false
}

async fn index() -> Json<Value> {
    Json(json!({
        "message": "Hello, World!"
    }))
}

async fn upload_file(
    TypedHeader(authorization): TypedHeader<Authorization<Bearer>>,
    Query(UploadFileQuery { directory, safe }): Query<UploadFileQuery>,
    mut multipart: Multipart,
) -> Result<Json<Value>, StatusCode> {
    if authorization.token()
        != std::env::var("AUTH_TOKEN")
            .unwrap_or_else(|_| "aaa".to_string())
            .as_str()
    {
        return Err(StatusCode::UNAUTHORIZED);
    }

    if let Ok(Some(mut field)) = multipart.next_field().await {
        if field.file_name().is_some() {
            let filename = field
                .file_name()
                .unwrap_or_else(|| unreachable!())
                .to_string();
            let directory = directory
                .map(|d| {
                    if d == "/" {
                        return Ok(d);
                    }
                    if d.contains("/") {
                        return Err(StatusCode::BAD_REQUEST);
                    }
                    Ok(format!("/{}/", d.trim_matches('/')))
                })
                .unwrap_or_else(|| Ok("/".to_string()))?;

            let base_path = format!("{}{}", directory, filename);
            let path_string = format!("/home/services/cdn/uploads{}", base_path);
            let path = StdPath::new(&path_string);
            if safe && path.exists() {
                return Err(StatusCode::CONFLICT);
            }

            let mut buffer: Vec<u8> = Vec::new();
            let mut file_size: u64 = 0;

            while let Some(chunk) = field.next().await {
                let data = chunk.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

                file_size += data.len() as u64;

                if file_size > 35 * 1024 * 1024 {
                    return Err(StatusCode::PAYLOAD_TOO_LARGE);
                }

                buffer.extend_from_slice(&data);
            }

            fs::create_dir_all(path.parent().unwrap())
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            fs::write(path, &buffer[..])
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            return Ok(Json(json!({
                "message": "File uploaded",
                "filename": filename,
                "directory": directory,
                "path": base_path,
            })));
        } else {
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    Err(StatusCode::BAD_REQUEST)
}

async fn handle_get_file(filename: String, path: String) -> Result<Response<BoxBody>, StatusCode> {
    let file = fs::read(path).await.map_err(|e| match e.kind() {
        NotFound => StatusCode::NOT_FOUND,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
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
            )
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
        )
        .body(body::boxed(body::Full::from(file)))
        .unwrap_or_else(|_e| unreachable!("{_e:?}"));

    Ok(resp)
}

async fn get_file(Path(filename): Path<String>) -> Result<Response<BoxBody>, StatusCode> {
    let path = format!("/home/services/cdn/uploads/{}", filename);

    handle_get_file(filename, path).await
}

async fn get_paste(Path(filename): Path<String>) -> Result<Response<BoxBody>, StatusCode> {
    let path = format!("/home/services/cdn/uploads/{}", filename);
    let text = match String::from_utf8(fs::read(path).await.map_err(|e| match e.kind() {
        NotFound => StatusCode::NOT_FOUND,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    })?) {
        Ok(s) => s,
        Err(_) => {
            return Err(StatusCode::NOT_FOUND);
        }
    };
    let sanitized = filename.trim_matches('/').split('/').last().unwrap();

    let html = format!(
        r#"
            <html><head><link rel="stylesheet" href="//cdnjs.cloudflare.com/ajax/libs/highlight.js/11.5.1/styles/atom-one-dark.min.css">
            <script src="//cdnjs.cloudflare.com/ajax/libs/highlight.js/11.5.1/highlight.min.js"></script><style>@import url(
            'https://fonts.googleapis.com/css2?family=Inter:wght@200;300;400;500;600;700;800;900&family=JetBrains+Mono:ital,wght@0,100;0,200;0,300;0,400;0,500;0,600;0,700;0,800;1,100;1,200;1,300;1,400;1,500;1,600;1,700;1,800&display=swap'
            ); *{{font-family: 'Jetbrains Mono', monospace;}}body{{margin: 0; background: #282c34;color:white}}pre{{margin: 0; padding: 16px; font-size: 14px; min-height: 100%;}}div
            {{display: flex; align-items: center;}}div div{{display: flex; align-items: center; margin-left: 16px; flex-grow: 1; padding-top: 12px;}}button
            {{border: none; cursor: pointer; color: white; background-color: #14a84c; border-radius: 4px; font-size: 14px; margin: 6px; padding: 8px; min-width: 64px;
            opacity: 0.75; transition: all 0.3s ease;}}button:hover{{opacity: 1; background-color: #17bd56;}}h2{{margin: 16px 0 0 16px; font-weight: 400; 
            color: rgba(255, 255, 255, 0.3); font-size: 1.2em;}}h2, button{{-webkit-user-select: none; -moz-user-select: none; -ms-user-select: none; user-select: none;}}
            h2 #second{{color: rgba(255, 255, 255, 0.33); font-weight: 500;}}</style></head><body><div> <h2> <span id="first">Paste: </span> <span id="second"><i>{}</i></span>
            </h2> <div><button onclick="copyButton()" id="copy">Copy</button> <button onclick="copyLink()" id="copylink">Copy Link</button></div></div>
            <pre style="word-wrap: break-word; white-space: pre-wrap;" id="o">{}</pre><script>document.querySelectorAll('pre').forEach(el=>{{hljs.highlightElement(el);}});
            function copy(text){{navigator.clipboard.writeText(text);}}function copyButton(){{copy(document.getElementById('o').innerText); let el=document.getElementById('copy');
            el.innerHTML='Copied!'; setTimeout(()=>{{el.innerHTML='Copy';}}, 1000);}}function copyLink(){{copy(window.location.href); let el=document.getElementById('copylink');
            el.innerHTML='Copied Link!'; setTimeout(()=>{{el.innerHTML='Copy Link';}},1000);}}</script> </body></html>
        "#,
        sanitized, text,
    );

    let resp = Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, HeaderValue::from_static("text/html"))
        .body(body::boxed(body::Full::from(html)))
        .unwrap_or_else(|_e| unreachable!("{_e:?}"));

    Ok(resp)
}

async fn handle_delete_file(path: String) -> Result<Json<Value>, StatusCode> {
    fs::remove_file(path).await.map_err(|e| match e.kind() {
        NotFound => StatusCode::NOT_FOUND,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    })?;

    Ok(Json(json!({
        "message": "File deleted"
    })))
}

async fn delete_file(Path(filename): Path<String>) -> Result<Json<Value>, StatusCode> {
    let path = format!("/home/services/cdn/uploads/{}", filename);

    handle_delete_file(path).await
}

#[tokio::main]
async fn main() {
    dotenv().ok();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "tower_http=info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let _ = fs::create_dir_all("/home/services/cdn/uploads".to_string())
        .await
        .map_err(|e| match e.kind() {
            AlreadyExists => (),
            _ => panic!("{e:?}"),
        });

    let router = Router::new()
        .route("/", get(index))
        .route("/upload", post(upload_file))
        .route("/uploads/*filename", get(get_file).delete(delete_file))
        .route("/pastes/*filename", get(get_paste))
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
