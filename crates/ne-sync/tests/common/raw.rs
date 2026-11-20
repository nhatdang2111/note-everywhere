//! A bare WebSocket speaking the wire protocol, for tests that need to send
//! what the engine never would (duplicates, out-of-order requests, garbage).

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use ne_sync::SyncClient;
use ne_sync_protocol::{ClientFrame, ServerFrame};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderValue, header};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use url::Url;

use super::USER_AGENT;

pub struct RawSocket(WebSocketStream<MaybeTlsStream<TcpStream>>);

impl RawSocket {
    pub async fn connect(client: &SyncClient, base: &Url, device: &str) -> RawSocket {
        let session = client.session().expect("signed in");
        let mut url = base.clone();
        url.set_scheme("ws").expect("scheme");
        url.set_path("/v1/sync");
        url.query_pairs_mut().append_pair("device", device);
        let mut request = url.as_str().into_client_request().expect("request");
        let bearer = format!("Bearer {}", session.token.expose());
        request.headers_mut().insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&bearer).expect("header"),
        );
        request
            .headers_mut()
            .insert(header::USER_AGENT, HeaderValue::from_static(USER_AGENT));
        let (ws, _) = tokio_tungstenite::connect_async(request)
            .await
            .expect("connect");
        RawSocket(ws)
    }

    pub async fn send(&mut self, frame: &ClientFrame) {
        let bytes = frame.encode().expect("encode");
        self.send_raw(bytes).await;
    }

    /// Sends the application keepalive and returns what comes back as text.
    pub async fn ping(&mut self) -> String {
        self.0.send(Message::text("ping")).await.expect("send ping");
        loop {
            let next = tokio::time::timeout(Duration::from_secs(10), self.0.next())
                .await
                .expect("timed out waiting for the keepalive answer");
            match next {
                Some(Ok(Message::Text(text))) => return text.to_string(),
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    pub async fn send_raw(&mut self, bytes: Vec<u8>) {
        self.0.send(Message::binary(bytes)).await.expect("send");
    }

    /// The next protocol frame; `None` once the server closes the socket.
    pub async fn recv(&mut self) -> Option<ServerFrame> {
        loop {
            let next = tokio::time::timeout(Duration::from_secs(10), self.0.next())
                .await
                .expect("timed out waiting for a frame");
            match next {
                Some(Ok(Message::Binary(bytes))) => {
                    return Some(ServerFrame::decode(&bytes).expect("valid server frame"));
                }
                Some(Ok(Message::Text(_) | Message::Ping(_) | Message::Pong(_))) => {}
                _ => return None,
            }
        }
    }

    /// Pulls from `cursor` until done, returning every entry in order.
    pub async fn pull_all(
        &mut self,
        req_id: u32,
        mut cursor: u64,
    ) -> Vec<ne_sync_protocol::LogEntry> {
        let mut entries = Vec::new();
        loop {
            self.send(&ClientFrame::Pull { req_id, cursor }).await;
            match self.recv().await.expect("page") {
                ServerFrame::Updates {
                    done,
                    next_cursor,
                    entries: page,
                    ..
                } => {
                    entries.extend(page);
                    if done {
                        return entries;
                    }
                    cursor = next_cursor;
                }
                other => panic!("unexpected frame {other:?}"),
            }
        }
    }
}
