use axum::{
    Router,
    body::Body,
    extract::{
        State as AxumState, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::get,
};
use rust_embed::RustEmbed;
use tokio::{
    net::TcpListener,
    sync::{broadcast, oneshot},
};

use crate::{
    cli::Args,
    state::{FileSyncProgress, ProgressBroadcaster, StateBrokerMessage, StateBrokerTx},
    task::Task,
};

macro_rules! internal_error_response {
    () => {
        ErrorPayload {
            code: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
            message: "Internal error, see logs".to_string(),
        }
    };
}

#[derive(Clone)]
struct ServerState {
    state_tx: StateBrokerTx,
    state_bcast: ProgressBroadcaster,
}

impl ServerState {
    pub async fn get_all_sync_progress(&self) -> Result<Vec<FileSyncProgress>, ErrorPayload> {
        let (reply_tx, reply_rx) = oneshot::channel();

        self.state_tx
            .send(StateBrokerMessage::GetAllSyncProgress(reply_tx))
            .await
            .map_err(|e| {
                log::error!("failed to send message to state broker: {e:?}");
                internal_error_response!()
            })?;

        reply_rx.await.map_err(|e| {
            log::error!("no response received from state broker: {e:?}");
            internal_error_response!()
        })
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
enum WsMessage {
    SyncProgress(FileSyncProgress),
    CurrentStatus(AllProgressPayload),
    Response {
        id: String,
        #[serde(flatten)]
        body: ResponseBody,
    },

    Error(ErrorPayload),
}

impl WsMessage {
    async fn send(&self, socket: &mut WebSocket) {
        let json = match serde_json::to_string(&self) {
            Ok(json) => json,
            Err(e) => {
                log::error!("failed to serialize WsMessage: {e:?}");
                return;
            }
        };

        let msg = Message::Text(json.into());

        if let Err(e) = socket.send(msg).await {
            log::error!("failed to send WsMessage: {e:?}");
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
enum ResponseBody {
    CurrentStatus(AllProgressPayload),
    Error(ErrorPayload),
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
struct Request {
    id: String,
    #[serde(flatten)]
    body: RequestBody,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
enum RequestBody {
    GetCurrentStatus,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AllProgressPayload(Vec<FileSyncProgress>);

#[derive(Debug, Clone, serde::Serialize)]
pub struct ErrorPayload {
    pub code: u16,
    pub message: String,
}

#[derive(RustEmbed)]
#[folder = "src/frontend/dist/"]
struct Asset;

pub(crate) async fn init_http_server(
    args: &Args,
    state_tx: StateBrokerTx,
    state_bcast: ProgressBroadcaster,
) -> anyhow::Result<Task> {
    let router = Router::new()
        .fallback(get(static_handler))
        .route("/ws", get(ws_handler))
        .with_state(ServerState {
            state_tx,
            state_bcast,
        });

    let listener = TcpListener::bind(&args.http_addr).await?;
    log::info!("websocket server running on {}", args.http_addr);

    Ok(Box::pin(async move {
        axum::serve(listener, router).await?;
        Ok(())
    }))
}

async fn static_handler(uri: Uri) -> impl IntoResponse {
    let mut path = uri.path().trim_start_matches('/').to_string();

    if path.is_empty() {
        path = "index.html".to_string();
    }

    serve_asset(&path)
        // Default to index.html for client-side routing
        .or_else(|_| serve_asset("index.html"))
        .unwrap_or_else(|err| {
            log::error!(
                "failed to serve static asset at path {}: {}",
                uri.path(),
                err
            );
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        })
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    AxumState(state): AxumState<ServerState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(state, socket))
}

async fn handle_socket(state: ServerState, mut socket: WebSocket) {
    log::info!("established websocket connection with gui client");
    connection_setup(&state, &mut socket).await;

    let mut bcast_rx = state.state_bcast.subscribe();
    loop {
        tokio::select! {
            sync_progress = bcast_rx.recv() => {
                handle_sync_progress(&mut socket, sync_progress).await
            }
            client_msg = socket.recv() => {
                if handle_client_msg(&state, &mut socket, client_msg).await {
                    break
                }
            }
        }
    }
}

async fn connection_setup(state: &ServerState, socket: &mut WebSocket) {
    match state.get_all_sync_progress().await {
        Ok(all_progress) => {
            WsMessage::CurrentStatus(AllProgressPayload(all_progress))
                .send(socket)
                .await;
        }
        Err(err_payload) => {
            WsMessage::Error(err_payload).send(socket).await;
        }
    }
}

async fn handle_sync_progress(
    socket: &mut WebSocket,
    res: Result<FileSyncProgress, broadcast::error::RecvError>,
) {
    match res {
        Ok(sync_progress) => {
            WsMessage::SyncProgress(sync_progress).send(socket).await;
        }
        Err(e) => {
            log::error!("failed receiving sync progress from state broker: {e:?}");
        }
    }
}

/// Returns true if the connection should be closed.
async fn handle_client_msg(
    state: &ServerState,
    socket: &mut WebSocket,
    res: Option<Result<Message, axum::Error>>,
) -> bool {
    match res {
        Some(Ok(Message::Close(_))) | None => {
            log::debug!("peer initiated websocket close");
            return true;
        }
        Some(Ok(Message::Text(text))) => {
            let req: Request = match serde_json::from_str(&text) {
                Ok(req) => req,
                Err(e) => {
                    log::warn!("failed to deserialize incoming client request: {e}");
                    WsMessage::Error(ErrorPayload {
                        code: StatusCode::BAD_REQUEST.as_u16(),
                        message: format!("Invalid JSON request: {e}"),
                    })
                    .send(socket)
                    .await;

                    return false;
                }
            };

            WsMessage::Response {
                id: req.id,
                body: process_request(state, &req.body).await,
            }
            .send(socket)
            .await;
        }
        Some(Ok(Message::Ping(payload))) => {
            if socket.send(Message::Pong(payload)).await.is_err() {
                return true;
            }
        }
        Some(Err(e)) => {
            log::error!("websocket error: {e}");
            return true;
        }
        _ => return false,
    }

    false
}

async fn process_request(state: &ServerState, body: &RequestBody) -> ResponseBody {
    match body {
        RequestBody::GetCurrentStatus => match state.get_all_sync_progress().await {
            Ok(progress) => ResponseBody::CurrentStatus(AllProgressPayload(progress)),
            Err(err) => ResponseBody::Error(err),
        },
    }
}

fn get_mime_type(path: &str) -> &'static str {
    match path {
        p if p.ends_with(".js") => "text/javascript; charset=utf-8",
        p if p.ends_with(".css") => "text/css; charset=utf-8",
        p if p.ends_with(".html") => "text/html; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn serve_asset(path: &str) -> Result<Response, String> {
    let gz_path = format!("{}.gz", path);

    let file =
        Asset::get(&gz_path).ok_or_else(|| format!("file not found in rust-embed: {}", gz_path))?;

    let mime = get_mime_type(path);

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_ENCODING, "gzip")
        .body(Body::from(file.data))
        .map_err(|e| format!("failed to build response: {}", e))
}
