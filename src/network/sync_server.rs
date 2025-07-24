use super::protocol::{SessionState, SyncMessage, SyncEvent, UserId, UserState};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, error, info, warn};
use std::sync::Arc;

type ClientSender = mpsc::UnboundedSender<SyncMessage>;

/// Sync server that coordinates multiple clients
pub struct SyncServer {
    session_state: Arc<RwLock<SessionState>>,
    clients: Arc<RwLock<HashMap<UserId, ClientSender>>>,
    broadcast_tx: broadcast::Sender<SyncMessage>,
    sequence_counter: Arc<RwLock<u64>>,
}

impl SyncServer {
    /// Create a new sync server
    pub fn new() -> Self {
        let (broadcast_tx, _) = broadcast::channel(1000);
        
        Self {
            session_state: Arc::new(RwLock::new(SessionState::new())),
            clients: Arc::new(RwLock::new(HashMap::new())),
            broadcast_tx,
            sequence_counter: Arc::new(RwLock::new(0)),
        }
    }
    
    /// Start the server on the given address
    pub async fn start(&self, addr: SocketAddr) -> Result<()> {
        let listener = TcpListener::bind(addr).await
            .with_context(|| format!("Failed to bind to {}", addr))?;
            
        info!("Sync server listening on {}", addr);
        info!("Clients can connect and start syncing playlist positions");
        
        // Start the display loop in background
        let session_state = self.session_state.clone();
        tokio::spawn(async move {
            Self::display_loop(session_state).await;
        });
        
        // Accept client connections
        while let Ok((stream, client_addr)) = listener.accept().await {
            info!("New client connected from: {}", client_addr);
            
            let session_state = self.session_state.clone();
            let clients = self.clients.clone();
            let broadcast_tx = self.broadcast_tx.clone();
            let mut broadcast_rx = self.broadcast_tx.subscribe();
            let sequence_counter = self.sequence_counter.clone();
            
            tokio::spawn(async move {
                if let Err(e) = Self::handle_client(
                    stream, 
                    client_addr,
                    session_state,
                    clients,
                    broadcast_tx,
                    &mut broadcast_rx,
                    sequence_counter,
                ).await {
                    error!("Client {} error: {}", client_addr, e);
                }
            });
        }
        
        Ok(())
    }
    
    /// Handle a single client connection
    async fn handle_client(
        stream: TcpStream,
        client_addr: SocketAddr,
        session_state: Arc<RwLock<SessionState>>,
        clients: Arc<RwLock<HashMap<UserId, ClientSender>>>,
        broadcast_tx: broadcast::Sender<SyncMessage>,
        broadcast_rx: &mut broadcast::Receiver<SyncMessage>,
        sequence_counter: Arc<RwLock<u64>>,
    ) -> Result<()> {
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        
        let (client_tx, mut client_rx) = mpsc::unbounded_channel::<SyncMessage>();
        let mut user_id: Option<UserId> = None;
        
        // Handle incoming messages from client
        let broadcast_tx_clone = broadcast_tx.clone();
        let session_state_clone = session_state.clone();
        let clients_clone = clients.clone();
        let sequence_counter_clone = sequence_counter.clone();
        
        tokio::spawn(async move {
            let mut line = String::new();
            
            while let Ok(bytes_read) = reader.read_line(&mut line).await {
                if bytes_read == 0 {
                    break; // Connection closed
                }
                
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    line.clear();
                    continue;
                }
                
                match serde_json::from_str::<SyncMessage>(trimmed) {
                    Ok(message) => {
                        debug!("Received from {}: {:?}", client_addr, message);
                        
                        // Update session state
                        match &message.event {
                            SyncEvent::UserJoined { user_id: uid, user_state } => {
                                debug!("Processing UserJoined for: {}", uid);
                                user_id = Some(uid.clone());
                                clients_clone.write().await.insert(uid.clone(), client_tx.clone());
                                session_state_clone.write().await.update_user(user_state.clone());
                            }
                            SyncEvent::StateUpdate { user_state } => {
                                debug!("Processing StateUpdate for user: {}, pos: {}, file: {:?}", 
                                       user_state.user_id, user_state.playlist_position, user_state.current_file_name);
                                session_state_clone.write().await.update_user(user_state.clone());
                            }
                            SyncEvent::UserLeft { user_id: uid } => {
                                debug!("Processing UserLeft for: {}", uid);
                                clients_clone.write().await.remove(uid);
                                session_state_clone.write().await.remove_user(uid);
                            }
                            _ => {}
                        }
                        
                        // Broadcast to all other clients
                        if let Err(e) = broadcast_tx_clone.send(message) {
                            warn!("Failed to broadcast message: {}", e);
                        }
                    }
                    Err(e) => {
                        warn!("Failed to parse message from {}: {} - '{}'", client_addr, e, trimmed);
                    }
                }
                
                line.clear();
            }
            
            // Clean up when client disconnects
            if let Some(uid) = user_id {
                info!("Client {} ({}) disconnected", client_addr, uid);
                clients_clone.write().await.remove(&uid);
                session_state_clone.write().await.remove_user(&uid);
                
                // Send user left message
                let mut seq = sequence_counter_clone.write().await;
                *seq += 1;
                let leave_message = SyncMessage::user_left(uid, *seq);
                let _ = broadcast_tx_clone.send(leave_message);
            }
        });
        
        // Handle outgoing messages to client
        loop {
            tokio::select! {
                // Receive message to send to this client
                msg = client_rx.recv() => {
                    match msg {
                        Some(message) => {
                            let json = serde_json::to_string(&message)?;
                            if let Err(e) = writer.write_all(format!("{}
", json).as_bytes()).await {
                                error!("Failed to write to client {}: {}", client_addr, e);
                                break;
                            }
                        }
                        None => break, // Channel closed
                    }
                }
                
                // Receive broadcast message to forward to client
                msg = broadcast_rx.recv() => {
                    match msg {
                        Ok(message) => {
                            let json = serde_json::to_string(&message)?;
                            if let Err(e) = writer.write_all(format!("{}
", json).as_bytes()).await {
                                error!("Failed to write broadcast to client {}: {}", client_addr, e);
                                break;
                            }
                        }
                        Err(e) => {
                            debug!("Broadcast receive error: {}", e);
                            break;
                        }
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Display loop showing current session state, now with auto-refresh.
    async fn display_loop(session_state: Arc<RwLock<SessionState>>) {
        use tokio::time::{interval, Duration};

        let mut interval = interval(Duration::from_millis(500)); // Faster refresh

        loop {
            interval.tick().await;

            let state = session_state.read().await;
            let display_lines = state.format_for_display();
            let summary = state.get_sync_summary();

            // ANSI escape code to clear screen and move cursor to top-left
            print!("[2J[1;1H");

            if !state.users.is_empty() {
                println!("🎬 SyncRead Server - {}", summary);
                println!("{}", "=".repeat(60));

                for line in display_lines {
                    println!("{}", line);
                }

                println!("{}", "=".repeat(60));
            } else {
                println!("🎬 SyncRead Server");
                println!("{}", "=".repeat(60));
                println!("Waiting for clients to connect...");
                println!(
                    "Run client with: syncread client --server <IP>:8080 --user-id <name> <files...>"
                );
            }

            println!("
Press Ctrl+C to stop the server");
        }
    }
}

impl Default for SyncServer {
    fn default() -> Self {
        Self::new()
    }
}