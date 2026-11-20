//! A TCP proxy in front of the local server that can break connections on
//! demand, to test what sync does when the network fails mid-push.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use url::Url;

#[derive(Default)]
struct Control {
    /// Bumped to drop every open connection.
    generation: AtomicU64,
    /// When set, the next client data after the WebSocket handshake reaches
    /// the server but its answer never returns, then the link dies.
    cut_next: AtomicBool,
    /// Connections accepted so far.
    accepted: AtomicU64,
    /// While set, new connections are refused and open ones are dropped.
    blocked: AtomicBool,
}

pub struct Proxy {
    addr: SocketAddr,
    control: Arc<Control>,
    task: JoinHandle<()>,
}

impl Proxy {
    pub async fn start(target: SocketAddr) -> Proxy {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind proxy");
        let addr = listener.local_addr().expect("proxy address");
        let control = Arc::new(Control::default());
        let shared = control.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((client, _)) = listener.accept().await else {
                    return;
                };
                if shared.blocked.load(Ordering::SeqCst) {
                    continue;
                }
                shared.accepted.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(relay(client, target, shared.clone()));
            }
        });
        Proxy {
            addr,
            control,
            task,
        }
    }

    pub fn url(&self) -> Url {
        Url::parse(&format!("http://{}", self.addr)).expect("proxy url")
    }

    /// Drops every open connection right now.
    pub fn kill_connections(&self) {
        self.control.generation.fetch_add(1, Ordering::SeqCst);
    }

    /// Simulates losing the network: connections drop and stay refused.
    pub fn block(&self) {
        self.control.blocked.store(true, Ordering::SeqCst);
        self.kill_connections();
    }

    pub fn unblock(&self) {
        self.control.blocked.store(false, Ordering::SeqCst);
    }

    /// Lets the next frame through but loses its acknowledgement.
    pub fn lose_next_ack(&self) {
        self.control.cut_next.store(true, Ordering::SeqCst);
    }

    pub fn cut_pending(&self) -> bool {
        self.control.cut_next.load(Ordering::SeqCst)
    }

    pub fn connections(&self) -> u64 {
        self.control.accepted.load(Ordering::SeqCst)
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn relay(mut client: TcpStream, target: SocketAddr, control: Arc<Control>) {
    let Ok(mut server) = TcpStream::connect(target).await else {
        return;
    };
    let generation = control.generation.load(Ordering::SeqCst);
    let (mut client_r, mut client_w) = client.split();
    let (mut server_r, mut server_w) = server.split();
    let (mut from_client, mut from_server) = (vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]);
    let mut handshake_done = false;
    let mut blackhole = false;
    let mut tick = tokio::time::interval(Duration::from_millis(10));
    let mut cut_deadline: Option<tokio::time::Instant> = None;
    loop {
        tokio::select! {
            read = client_r.read(&mut from_client) => {
                let Ok(n) = read else { return };
                if n == 0 || server_w.write_all(&from_client[..n]).await.is_err() { return }
                if handshake_done && !blackhole && control.cut_next.swap(false, Ordering::SeqCst) {
                    blackhole = true;
                    cut_deadline = Some(tokio::time::Instant::now() + Duration::from_millis(150));
                }
            }
            read = server_r.read(&mut from_server) => {
                let Ok(n) = read else { return };
                if n == 0 { return }
                handshake_done = true;
                if !blackhole && client_w.write_all(&from_server[..n]).await.is_err() { return }
            }
            _ = tick.tick() => {
                if control.generation.load(Ordering::SeqCst) != generation { return }
                if cut_deadline.is_some_and(|at| tokio::time::Instant::now() >= at) { return }
            }
        }
    }
}
