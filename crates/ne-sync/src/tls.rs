//! TLS configuration shared by HTTP and WebSocket connections: rustls with the
//! `ring` provider and the operating system's certificate verifier, so no
//! C toolchain is needed on Windows and enterprise roots keep working.

use std::sync::Arc;

use rustls::ClientConfig;
use rustls_platform_verifier::BuilderVerifierExt;

use crate::error::{Result, SyncError};

pub fn client_config() -> Result<Arc<ClientConfig>> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .and_then(|builder| builder.with_platform_verifier())
        .map_err(|err| SyncError::Network(format!("TLS setup failed: {err}")))?
        .with_no_client_auth();
    Ok(Arc::new(config))
}
