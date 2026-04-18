use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{
        sse::{Event, Sse},
        IntoResponse,
    },
    routing::{get, post},
    Json, Router,
};
use futures::stream::Stream;
use serde::Deserialize;
use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tower_http::cors::CorsLayer;

use crate::state::{AppState, SignalingMessage, SseEvent};

/// 建立 Axum Router
pub fn create_router(state: AppState, client_dist_dir: Option<String>) -> Router {
    let api = Router::new()
        .route("/api/files", get(list_files))
        .route("/api/files/:id/download", get(download_file))
        .route("/api/files/:id/request", post(request_download))
        .route("/api/files/:id/complete", post(mark_complete))
        .route("/api/files/:id/relay-complete", post(relay_complete))
        .route("/api/files/:id/transfer-failed", post(transfer_failed))
        .route("/api/events", get(sse_handler))
        .route("/api/signaling/offer", post(signaling_offer))
        .route("/api/signaling/answer", post(signaling_answer))
        .route(
            "/api/signaling/ice-candidate",
            post(signaling_ice_candidate),
        )
        .route("/api/relay/register", post(register_relay))
        .layer(CorsLayer::permissive())
        .with_state(state);

    if let Some(dir) = client_dist_dir {
        // 提供下載端 SPA 靜態檔案
        let index_path = format!("{}/client.html", dir);
        let serve = tower_http::services::ServeDir::new(&dir)
            .fallback(tower_http::services::ServeFile::new(index_path));
        api.fallback_service(serve)
    } else {
        api
    }
}

/// GET /api/files — 回傳檔案清單
async fn list_files(State(state): State<AppState>) -> Json<Vec<crate::state::FileInfo>> {
    Json(state.list_files().await)
}

/// GET /api/files/:id/download — 串流下載檔案
async fn download_file(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let file = match state.get_file(&id).await {
        Some(f) => f,
        None => return Err(StatusCode::NOT_FOUND),
    };

    let file_handle = match tokio::fs::File::open(&file.path).await {
        Ok(f) => f,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let stream = tokio_util::io::ReaderStream::new(file_handle);
    let body = axum::body::Body::from_stream(stream);

    Ok((
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/octet-stream".to_string(),
            ),
            (
                axum::http::header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", file.name),
            ),
            (axum::http::header::CONTENT_LENGTH, file.size.to_string()),
        ],
        body,
    ))
}

/// POST /api/files/:id/request — 下載請求
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadRequest {
    client_id: String,
}

async fn request_download(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<DownloadRequest>,
) -> impl IntoResponse {
    match state.request_download(&id, &body.client_id).await {
        Some(event) => Ok(Json(event)),
        None => Err(StatusCode::NOT_FOUND),
    }
}

/// POST /api/files/:id/complete — 標記下載完成
async fn mark_complete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<DownloadRequest>,
) -> StatusCode {
    state.mark_download_complete(&id, &body.client_id).await;
    StatusCode::OK
}

/// POST /api/files/:id/relay-complete — 中繼傳輸完成
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RelayCompleteRequest {
    source_client_id: String,
    target_client_id: String,
}

async fn relay_complete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<RelayCompleteRequest>,
) -> StatusCode {
    state
        .mark_relay_complete(&id, &body.source_client_id, &body.target_client_id)
        .await;
    StatusCode::OK
}

/// POST /api/files/:id/transfer-failed — 傳輸失敗
async fn transfer_failed(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<DownloadRequest>,
) -> StatusCode {
    state.mark_transfer_failed(&id, &body.client_id).await;
    StatusCode::OK
}

/// GET /api/events — SSE 端點
async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // 註冊新的下載端
    let client_id = state.register_client().await;

    // 準備狀態快照
    let files = state.list_files().await;
    let snapshot = serde_json::json!({
        "clientId": client_id,
        "files": files,
        "schedules": [],
    });

    let rx = state.sse_tx.subscribe();
    let broadcast_stream = BroadcastStream::new(rx);

    // 先推送快照，再推送後續事件
    let snapshot_event = Event::default()
        .event("snapshot")
        .json_data(&snapshot)
        .unwrap();

    let initial = futures::stream::once(async move { Ok::<_, Infallible>(snapshot_event) });

    let events = broadcast_stream.filter_map(move |result| match result {
        Ok(sse_event) => {
            let event = Event::default()
                .event(&sse_event.event_type)
                .json_data(&sse_event.data)
                .ok()?;
            Some(Ok(event))
        }
        Err(_) => None,
    });

    let inner_stream = initial.chain(events);

    // 包裝 stream 以偵測客戶端斷線（stream drop 時清理）
    let cleanup_stream = ClientStream {
        inner: Box::pin(inner_stream),
        state: state.clone(),
        client_id: client_id.clone(),
        cleaned_up: false,
    };

    Sse::new(cleanup_stream).keep_alive(axum::response::sse::KeepAlive::default())
}

/// 包裝 SSE stream，當 stream 被 drop（客戶端斷線）時執行清理
struct ClientStream<S> {
    inner: Pin<Box<S>>,
    state: AppState,
    client_id: String,
    cleaned_up: bool,
}

impl<S: Stream<Item = Result<Event, Infallible>>> Stream for ClientStream<S> {
    type Item = Result<Event, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

impl<S> Drop for ClientStream<S> {
    fn drop(&mut self) {
        if !self.cleaned_up {
            self.cleaned_up = true;
            let state = self.state.clone();
            let client_id = self.client_id.clone();
            println!("下載端斷線: {}", client_id);
            tokio::spawn(async move {
                state.disconnect_client(&client_id).await;
            });
        }
    }
}

/// POST /api/signaling/offer — WebRTC SDP offer 轉發
async fn signaling_offer(
    State(state): State<AppState>,
    Json(msg): Json<SignalingMessage>,
) -> StatusCode {
    state.broadcast(SseEvent {
        event_type: "signaling".to_string(),
        data: serde_json::to_value(&msg).unwrap(),
    });
    StatusCode::OK
}

/// POST /api/signaling/answer — WebRTC SDP answer 轉發
async fn signaling_answer(
    State(state): State<AppState>,
    Json(msg): Json<SignalingMessage>,
) -> StatusCode {
    state.broadcast(SseEvent {
        event_type: "signaling".to_string(),
        data: serde_json::to_value(&msg).unwrap(),
    });
    StatusCode::OK
}

/// POST /api/signaling/ice-candidate — ICE 候選轉發
async fn signaling_ice_candidate(
    State(state): State<AppState>,
    Json(msg): Json<SignalingMessage>,
) -> StatusCode {
    state.broadcast(SseEvent {
        event_type: "signaling".to_string(),
        data: serde_json::to_value(&msg).unwrap(),
    });
    StatusCode::OK
}

/// POST /api/relay/register — 中繼端註冊
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RelayRegisterRequest {
    client_id: String,
    file_id: String,
}

async fn register_relay(
    State(state): State<AppState>,
    Json(body): Json<RelayRegisterRequest>,
) -> StatusCode {
    let mut clients = state.clients.write().await;
    if let Some(client) = clients.get_mut(&body.client_id) {
        if !client.completed_files.contains(&body.file_id) {
            client.completed_files.push(body.file_id.clone());
        }
    }
    StatusCode::OK
}
