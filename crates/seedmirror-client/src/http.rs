use axum::{
    Router,
    extract::{
        State as AxumState, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use tokio::{
    net::TcpListener,
    sync::{broadcast, oneshot},
};

use crate::{
    cli::Args,
    state::{FileSyncProgress, ProgressBroadcaster, StateBrokerMessage, StateBrokerTx},
    task::Task,
};

#[derive(Clone)]
struct ServerState {
    state_tx: StateBrokerTx,
    state_bcast: ProgressBroadcaster,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum WsMessage {
    SyncProgress(FileSyncProgress),
    CurrentStatus(AllProgressPayload),

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
pub struct AllProgressPayload(Vec<FileSyncProgress>);

#[derive(Debug, Clone, serde::Serialize)]
pub struct ErrorPayload {
    pub code: u16,
    pub message: String,
}

macro_rules! internal_error_response {
    () => {
        ErrorPayload {
            code: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
            message: "Internal error, see logs".to_string(),
        }
    };
}

pub(crate) async fn init_http_server(
    args: &Args,
    state_tx: StateBrokerTx,
    state_bcast: ProgressBroadcaster,
) -> anyhow::Result<Task> {
    let router = Router::new()
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
                if handle_client_msg(&mut socket, client_msg).await {
                    break
                }
            }
        }
    }
}

async fn connection_setup(state: &ServerState, socket: &mut WebSocket) {
    let (reply_tx, reply_rx) = oneshot::channel();

    if let Err(e) = state
        .state_tx
        .send(StateBrokerMessage::GetAllSyncProgress(reply_tx))
        .await
    {
        log::error!("failed to request path status from state broker: {e:?}");
        WsMessage::Error(internal_error_response!())
            .send(socket)
            .await;

        return;
    }

    match reply_rx.await {
        Ok(all_progress) => {
            WsMessage::CurrentStatus(AllProgressPayload(all_progress))
                .send(socket)
                .await
        }
        Err(e) => {
            log::error!("no `GetAllSyncProgress` response from state broker: {e:?}");
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
    socket: &mut WebSocket,
    res: Option<Result<Message, axum::Error>>,
) -> bool {
    match res {
        Some(Ok(Message::Close(_))) | None => {
            log::debug!("peer initiated websocket close");
            return true;
        }
        Some(Ok(Message::Ping(payload))) => {
            if socket.send(Message::Pong(payload)).await.is_err() {
                return true;
            }
        }
        Some(Ok(Message::Text(text))) => {
            // Optional: Handle incoming client JSON requests here
            log::trace!("received from websocket peer: {text}");
            return false;
        }
        Some(Err(e)) => {
            log::error!("websocket error: {e}");
            return true;
        }
        _ => return false,
    }

    false
}
