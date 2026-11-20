//! A TCP relay in front of the test server that can be cut and restored, to
//! simulate losing the network.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

pub struct Proxy {
    pub addr: SocketAddr,
    cut: Arc<AtomicBool>,
    connections: Arc<Mutex<Vec<JoinHandle<()>>>>,
    accept: JoinHandle<()>,
}

impl Proxy {
    pub async fn start(upstream: SocketAddr) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind proxy");
        let addr = listener.local_addr().expect("addr");
        let cut = Arc::new(AtomicBool::new(false));
        let connections: Arc<Mutex<Vec<JoinHandle<()>>>> = Arc::default();
        let (flag, conns) = (cut.clone(), connections.clone());
        let accept = tokio::spawn(async move {
            while let Ok((mut client, _)) = listener.accept().await {
                if flag.load(Ordering::SeqCst) {
                    continue; // dropped: the client sees a closed connection
                }
                let task = tokio::spawn(async move {
                    if let Ok(mut server) = TcpStream::connect(upstream).await {
                        let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
                    }
                });
                conns.lock().expect("lock").push(task);
            }
        });
        Self {
            addr,
            cut,
            connections,
            accept,
        }
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Drops every open connection and refuses new ones.
    pub fn cut(&self) {
        self.cut.store(true, Ordering::SeqCst);
        for task in self.connections.lock().expect("lock").drain(..) {
            task.abort();
        }
    }

    pub fn restore(&self) {
        self.cut.store(false, Ordering::SeqCst);
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.accept.abort();
        for task in self.connections.lock().expect("lock").drain(..) {
            task.abort();
        }
    }
}
