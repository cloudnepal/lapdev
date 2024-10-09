use anyhow::Result;
use axum::{
    extract::{ws::WebSocket, WebSocketUpgrade},
    http::Uri,
    response::Response,
};
use futures::{SinkExt, StreamExt};

pub enum ProxyForward {
    Resp(Response),
    Proxy(Uri),
}

pub async fn handler(
    host: &str,
    path_query: &str,
    websocket: Option<WebSocketUpgrade>,
    port: Option<u16>,
) -> Option<ProxyForward> {
    let port = port?;
    if let Some(websocket) = websocket {
        println!("proxy ide websocket");
        let uri = format!("ws://{host}:{port}{}", path_query);
        let resp = websocket.on_upgrade(move |socket| async move {
            if let Err(e) = proxy_socket(socket, uri).await {
                println!("handle websocket error: {e}");
            } else {
                println!("handle websocket finished");
            }
        });
        return Some(ProxyForward::Resp(resp));
    }

    let uri = format!("http://{host}:{port}{path_query}");
    let uri = Uri::try_from(uri).ok()?;
    Some(ProxyForward::Proxy(uri))
}

async fn proxy_socket(socket: WebSocket, req: String) -> Result<()> {
    let (ws_stream, _) = tokio_tungstenite::connect_async(req).await?;
    let (mut server_sender, mut server_receiver) = ws_stream.split();

    let (mut client_sender, mut client_receiver) = socket.split();

    let mut server_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = server_receiver.next().await {
            if let Some(msg) = msg_from_tungstenite(msg) {
                let _ = client_sender.send(msg).await;
            }
        }
    });

    let mut client_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = client_receiver.next().await {
            let _ = server_sender.send(msg_into_tungstenite(msg)).await;
        }
    });

    //wait for either task to finish and kill the other task
    tokio::select! {
        _ = (&mut client_task) => {
            server_task.abort();
        },
        _ = (&mut server_task) => {
            client_task.abort();
        }
    }

    Ok(())
}

fn msg_into_tungstenite(
    msg: axum::extract::ws::Message,
) -> tokio_tungstenite::tungstenite::Message {
    match msg {
        axum::extract::ws::Message::Text(text) => {
            tokio_tungstenite::tungstenite::Message::Text(text)
        }
        axum::extract::ws::Message::Binary(binary) => {
            tokio_tungstenite::tungstenite::Message::Binary(binary)
        }
        axum::extract::ws::Message::Ping(ping) => {
            tokio_tungstenite::tungstenite::Message::Ping(ping)
        }
        axum::extract::ws::Message::Pong(pong) => {
            tokio_tungstenite::tungstenite::Message::Pong(pong)
        }
        axum::extract::ws::Message::Close(Some(close)) => {
            tokio_tungstenite::tungstenite::Message::Close(Some(
                tokio_tungstenite::tungstenite::protocol::CloseFrame {
                    code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::from(
                        close.code,
                    ),
                    reason: close.reason,
                },
            ))
        }
        axum::extract::ws::Message::Close(None) => {
            tokio_tungstenite::tungstenite::Message::Close(None)
        }
    }
}

fn msg_from_tungstenite(
    message: tokio_tungstenite::tungstenite::Message,
) -> Option<axum::extract::ws::Message> {
    match message {
        tokio_tungstenite::tungstenite::Message::Text(text) => {
            Some(axum::extract::ws::Message::Text(text))
        }
        tokio_tungstenite::tungstenite::Message::Binary(binary) => {
            Some(axum::extract::ws::Message::Binary(binary))
        }
        tokio_tungstenite::tungstenite::Message::Ping(ping) => {
            Some(axum::extract::ws::Message::Ping(ping))
        }
        tokio_tungstenite::tungstenite::Message::Pong(pong) => {
            Some(axum::extract::ws::Message::Pong(pong))
        }
        tokio_tungstenite::tungstenite::Message::Close(Some(close)) => Some(
            axum::extract::ws::Message::Close(Some(axum::extract::ws::CloseFrame {
                code: close.code.into(),
                reason: close.reason,
            })),
        ),
        tokio_tungstenite::tungstenite::Message::Close(None) => {
            Some(axum::extract::ws::Message::Close(None))
        }
        // we can ignore `Frame` frames as recommended by the tungstenite maintainers
        // https://github.com/snapview/tungstenite-rs/issues/268
        tokio_tungstenite::tungstenite::Message::Frame(_) => None,
    }
}
