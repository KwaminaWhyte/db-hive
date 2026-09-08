//! SSH Tunnel Management
//!
//! This module provides SSH tunneling functionality for secure database connections.
//! It supports both password and key-based authentication and manages tunnel lifecycle.

use crate::models::connection::{SshAuthMethod, SshConfig};
use crate::models::DbError;
use async_trait::async_trait;
use russh::client;
use russh_keys::key;
use russh_keys::key::PublicKey;
use std::collections::HashMap;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

/// Active SSH tunnel information
struct TunnelInfo {
    /// Local port where tunnel is listening
    local_port: u16,
    /// Task handle for the tunnel listener
    task_handle: JoinHandle<()>,
    /// SSH session handle
    session: Arc<Mutex<client::Handle<SshClientHandler>>>,
}

/// SSH client handler
///
/// Verifies the server's host key on a trust-on-first-use basis: the key
/// fingerprint is recorded the first time we connect to a host, and any
/// later connection presenting a different key is rejected.
struct SshClientHandler {
    /// "host:port" of the SSH server being connected to
    server_addr: String,
    /// Set to a human-readable reason when the host key is rejected,
    /// so `create_tunnel` can surface a useful error to the user
    rejection_reason: Arc<StdMutex<Option<String>>>,
}

#[async_trait]
impl client::Handler for SshClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &key::PublicKey,
    ) -> Result<bool, Self::Error> {
        match verify_host_key(&self.server_addr, server_public_key) {
            Ok(()) => Ok(true),
            Err(reason) => {
                eprintln!("SSH host key verification failed: {}", reason);
                *self.rejection_reason.lock().unwrap() = Some(reason);
                Ok(false)
            }
        }
    }
}

/// Path to the JSON file mapping "host:port" to trusted key fingerprints
fn known_hosts_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("db-hive").join("ssh_known_hosts.json"))
}

/// Verify a server host key against the stored fingerprint (TOFU)
///
/// Returns `Ok(())` if the key matches the stored fingerprint, or if this
/// is the first connection to the host (in which case the fingerprint is
/// recorded). Returns `Err(reason)` on mismatch or storage failure.
fn verify_host_key(server_addr: &str, key: &PublicKey) -> Result<(), String> {
    let fingerprint = key.fingerprint();
    let path = known_hosts_path()
        .ok_or_else(|| "Cannot locate application data directory".to_string())?;

    let mut hosts: HashMap<String, String> = match std::fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents)
            .map_err(|e| format!("Corrupt SSH known hosts file at {}: {}", path.display(), e))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
        Err(e) => return Err(format!("Cannot read SSH known hosts file: {}", e)),
    };

    match hosts.get(server_addr) {
        Some(stored) if *stored == fingerprint => Ok(()),
        Some(stored) => Err(format!(
            "Host key for {} has CHANGED (stored fingerprint {}, server presented {}). \
             This may indicate a man-in-the-middle attack. If the server's key was \
             legitimately rotated, remove the entry for this host from {} and reconnect.",
            server_addr,
            stored,
            fingerprint,
            path.display()
        )),
        None => {
            // First connection to this host: trust and record the key
            hosts.insert(server_addr.to_string(), fingerprint);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Cannot create data directory: {}", e))?;
            }
            let json = serde_json::to_string_pretty(&hosts)
                .map_err(|e| format!("Cannot serialize SSH known hosts: {}", e))?;
            std::fs::write(&path, json)
                .map_err(|e| format!("Cannot write SSH known hosts file: {}", e))?;
            Ok(())
        }
    }
}

/// SSH Tunnel Manager
///
/// Manages SSH tunnel connections for database access
#[derive(Clone)]
pub struct SshTunnelManager {
    /// Active tunnels indexed by connection ID
    tunnels: Arc<Mutex<HashMap<String, TunnelInfo>>>,
}

impl SshTunnelManager {
    /// Create a new SSH tunnel manager
    pub fn new() -> Self {
        Self {
            tunnels: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create and start an SSH tunnel
    ///
    /// # Arguments
    ///
    /// * `connection_id` - Unique identifier for this connection
    /// * `config` - SSH tunnel configuration
    /// * `ssh_password` - Password for SSH authentication (if using password auth)
    /// * `db_host` - Target database host (from SSH server's perspective)
    /// * `db_port` - Target database port
    ///
    /// # Returns
    ///
    /// The local port where the tunnel is listening
    pub async fn create_tunnel(
        &self,
        connection_id: String,
        config: &SshConfig,
        ssh_password: Option<String>,
        db_host: String,
        db_port: u16,
    ) -> Result<u16, DbError> {
        // Check if tunnel already exists
        {
            let tunnels = self.tunnels.lock().await;
            if tunnels.contains_key(&connection_id) {
                return Err(DbError::ConnectionError(format!(
                    "SSH tunnel already exists for connection {}",
                    connection_id
                )));
            }
        }

        // Determine local port
        let local_port = if config.local_port == 0 {
            // Auto-assign a free port
            self.find_free_port()?
        } else {
            config.local_port
        };

        // Create SSH client configuration
        let ssh_config = Arc::new(client::Config::default());
        let ssh_addr = format!("{}:{}", config.host, config.port);
        let rejection_reason: Arc<StdMutex<Option<String>>> = Arc::new(StdMutex::new(None));
        let sh = SshClientHandler {
            server_addr: ssh_addr.clone(),
            rejection_reason: rejection_reason.clone(),
        };

        // Connect to SSH server (host key is verified by the handler)
        let mut session = match client::connect(ssh_config, &ssh_addr, sh).await {
            Ok(session) => session,
            Err(e) => {
                // Prefer the host key rejection reason over russh's generic error
                let reason = rejection_reason.lock().unwrap().take();
                return Err(DbError::ConnectionError(match reason {
                    Some(r) => r,
                    None => format!("SSH connection failed: {}", e),
                }));
            }
        };

        // Authenticate
        match &config.auth_method {
            SshAuthMethod::Password => {
                let password = ssh_password.ok_or_else(|| {
                    DbError::AuthError("SSH password required but not provided".to_string())
                })?;

                let auth_result = session
                    .authenticate_password(&config.username, &password)
                    .await
                    .map_err(|e| DbError::AuthError(format!("SSH password authentication failed: {}", e)))?;

                if !auth_result {
                    return Err(DbError::AuthError("SSH authentication failed: incorrect password".to_string()));
                }
            }
            SshAuthMethod::PrivateKey => {
                let key_path = config.private_key_path.as_ref().ok_or_else(|| {
                    DbError::InvalidInput("Private key path required for key authentication".to_string())
                })?;

                // Load private key
                let key_data = std::fs::read(key_path)
                    .map_err(|e| DbError::ConnectionError(format!("Failed to read private key: {}", e)))?;

                let key = russh_keys::decode_secret_key(
                    std::str::from_utf8(&key_data)
                        .map_err(|e| DbError::AuthError(format!("Invalid UTF-8 in key file: {}", e)))?,
                    None,
                )
                .map_err(|e| DbError::AuthError(format!("Failed to parse private key: {}", e)))?;

                let auth_result = session
                    .authenticate_publickey(&config.username, Arc::new(key))
                    .await
                    .map_err(|e| DbError::AuthError(format!("SSH key authentication failed: {}", e)))?;

                if !auth_result {
                    return Err(DbError::AuthError("SSH key authentication failed".to_string()));
                }
            }
        }

        let session = Arc::new(Mutex::new(session));

        // Start local listener
        let listener = TcpListener::bind(format!("127.0.0.1:{}", local_port))
            .map_err(|e| DbError::ConnectionError(format!("Failed to bind local port {}: {}", local_port, e)))?;

        let actual_port = listener.local_addr()
            .map_err(|e| DbError::InternalError(format!("Failed to get local address: {}", e)))?
            .port();

        // Spawn tunnel forwarding task
        let session_clone = session.clone();
        let db_host_clone = db_host.clone();
        let task_handle = tokio::spawn(async move {
            Self::run_tunnel_listener(listener, session_clone, db_host_clone, db_port).await;
        });

        // Store tunnel info
        let tunnel_info = TunnelInfo {
            local_port: actual_port,
            task_handle,
            session,
        };

        self.tunnels.lock().await.insert(connection_id, tunnel_info);

        Ok(actual_port)
    }

    /// Run the tunnel listener loop
    async fn run_tunnel_listener(
        listener: TcpListener,
        session: Arc<Mutex<client::Handle<SshClientHandler>>>,
        db_host: String,
        db_port: u16,
    ) {
        // Set non-blocking mode
        listener.set_nonblocking(true).ok();

        loop {
            // Accept incoming connection
            let stream = match TcpListener::accept(&listener) {
                Ok((stream, _)) => stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    continue;
                }
                Err(e) => {
                    eprintln!("SSH tunnel listener error: {}", e);
                    break;
                }
            };

            // Convert to tokio TcpStream
            stream.set_nonblocking(false).ok();
            let tokio_stream = match TcpStream::from_std(stream) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Failed to convert stream: {}", e);
                    continue;
                }
            };

            // Spawn handler for this connection
            let session_clone = session.clone();
            let db_host_clone = db_host.clone();
            tokio::spawn(async move {
                if let Err(e) = Self::handle_tunnel_connection(tokio_stream, session_clone, db_host_clone, db_port).await {
                    eprintln!("Tunnel connection error: {}", e);
                }
            });
        }
    }

    /// Handle a single tunneled connection
    async fn handle_tunnel_connection(
        mut local_stream: TcpStream,
        session: Arc<Mutex<client::Handle<SshClientHandler>>>,
        db_host: String,
        db_port: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Open SSH channel for port forwarding
        let session_guard = session.lock().await;
        let mut channel = session_guard
            .channel_open_direct_tcpip(&db_host, db_port as u32, "127.0.0.1", 0)
            .await?;
        drop(session_guard);

        // Buffer for data transfer
        let mut local_buf = vec![0u8; 8192];

        loop {
            tokio::select! {
                // Read from local and write to remote
                result = local_stream.read(&mut local_buf) => {
                    match result {
                        Ok(0) => break, // Connection closed
                        Ok(n) => {
                            channel.data(&local_buf[..n]).await?;
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
                // Read from remote and write to local
                result = channel.wait() => {
                    match result {
                        Some(russh::ChannelMsg::Data { data }) => {
                            local_stream.write_all(&data).await?;
                        }
                        Some(russh::ChannelMsg::Eof) | None => break,
                        _ => {}
                    }
                }
            }
        }

        Ok(())
    }

    /// Find a free local port
    fn find_free_port(&self) -> Result<u16, DbError> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| DbError::InternalError(format!("Failed to find free port: {}", e)))?;

        let port = listener.local_addr()
            .map_err(|e| DbError::InternalError(format!("Failed to get local address: {}", e)))?
            .port();

        Ok(port)
    }

    /// Close a tunnel by connection ID
    pub async fn close_tunnel(&self, connection_id: &str) -> Result<(), DbError> {
        let mut tunnels = self.tunnels.lock().await;

        if let Some(tunnel_info) = tunnels.remove(connection_id) {
            // Abort the listener task
            tunnel_info.task_handle.abort();

            // Close the SSH session
            let session = tunnel_info.session.lock().await;
            session.disconnect(russh::Disconnect::ByApplication, "", "en").await
                .map_err(|e| DbError::InternalError(format!("Failed to disconnect SSH session: {}", e)))?;
        }

        Ok(())
    }

    /// Check if a tunnel exists for a connection
    pub async fn has_tunnel(&self, connection_id: &str) -> bool {
        let tunnels = self.tunnels.lock().await;
        tunnels.contains_key(connection_id)
    }

    /// Get the local port for a tunnel
    pub async fn get_local_port(&self, connection_id: &str) -> Option<u16> {
        let tunnels = self.tunnels.lock().await;
        tunnels.get(connection_id).map(|t| t.local_port)
    }
}
