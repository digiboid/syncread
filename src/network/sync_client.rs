use super::protocol::{SyncMessage, SyncEvent, UserId, UserState};
use crate::mpv::MpvController;
use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::net::TcpStream;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, warn};

/// Client that connects to sync server and synchronizes MPV state
pub struct SyncClient {
    user_id: UserId,
    sequence_counter: u64,
}

impl SyncClient {
    /// Create a new sync client
    pub fn new(user_id: UserId) -> Self {
        Self {
            user_id,
            sequence_counter: 0,
        }
    }
    
    /// Connect to sync server and start synchronization
    pub async fn connect_and_sync(
        &mut self,
        server_addr: SocketAddr,
        mut mpv_controller: MpvController,
        playlist_files: Vec<PathBuf>,
    ) -> Result<()> {
        info!("Connecting to sync server at {}", server_addr);
        
        let stream = TcpStream::connect(server_addr).await
            .with_context(|| format!("Failed to connect to {}", server_addr))?;
            
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        
        // Create channels for communication
        let (outgoing_tx, mut outgoing_rx) = mpsc::unbounded_channel::<SyncMessage>();
        
        info!("Connected to sync server as user: {}", self.user_id);
        
        // Send initial user joined message
        let initial_state = self.get_current_state(&mut mpv_controller, &playlist_files).await?;
        let join_message = SyncMessage::user_joined(
            self.user_id.clone(), 
            initial_state.clone(), 
            self.next_sequence()
        );
        
        self.send_message(&mut writer, join_message).await?;
        
        // Start periodic state updates
        let outgoing_tx_clone = outgoing_tx.clone();
        let user_id_clone = self.user_id.clone();
        let mut sequence_counter = self.sequence_counter;
        
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_millis(2500)); // Even slower updates to give MPV more time
            
            loop {
                interval.tick().await;
                
                match Self::get_current_state_with_user_id(&mut mpv_controller, &playlist_files, &user_id_clone).await {
                    Ok(state) => {
                        sequence_counter += 1;
                        let update_message = SyncMessage::state_update(state, sequence_counter);
                        
                        if let Err(e) = outgoing_tx_clone.send(update_message) {
                            error!("Failed to send state update: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        warn!("Failed to get MPV state: {}", e);
                    }
                }
            }
        });
        
        // Handle outgoing messages
        let user_id_for_cleanup = self.user_id.clone();
        tokio::spawn(async move {
            while let Some(message) = outgoing_rx.recv().await {
                if let Err(e) = Self::send_message_static(&mut writer, message).await {
                    error!("Failed to send message: {}", e);
                    break;
                }
            }
            
            // Send leave message when shutting down
            let leave_message = SyncMessage::user_left(user_id_for_cleanup, 999999);
            let _ = Self::send_message_static(&mut writer, leave_message).await;
        });
        
        // Handle incoming messages
        let mut line = String::new();
        while let Ok(bytes_read) = reader.read_line(&mut line).await {
            if bytes_read == 0 {
                info!("Server connection closed");
                break;
            }
            
            let trimmed = line.trim();
            if trimmed.is_empty() {
                line.clear();
                continue;
            }
            
            match serde_json::from_str::<SyncMessage>(trimmed) {
                Ok(message) => {
                    debug!("Received from server: {:?}", message);
                    self.handle_incoming_message(message).await;
                }
                Err(e) => {
                    warn!("Failed to parse server message: {} - '{}'", e, trimmed);
                }
            }
            
            line.clear();
        }
        
        Ok(())
    }
    
    /// Get current state from MPV controller
    async fn get_current_state(
        &self,
        mpv: &mut MpvController,
        playlist_files: &[PathBuf],
    ) -> Result<UserState> {
        // Add longer delays between requests to give MPV time to respond properly
        let playlist_pos = mpv.get_playlist_pos().await.unwrap_or(0);
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        
        let playback_time = mpv.get_position().await.unwrap_or(0.0);
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        
        let is_paused = mpv.is_paused().await.unwrap_or(true);
        
        let current_file = if playlist_pos >= 0 && (playlist_pos as usize) < playlist_files.len() {
            Some(playlist_files[playlist_pos as usize].clone())
        } else {
            None
        };
        
        let mut state = UserState::new(self.user_id.clone());
        state.update_from_mpv(playlist_pos, playback_time, is_paused, current_file);
        
        Ok(state)
    }
    
    /// Static version for use in spawned tasks with proper user_id
    async fn get_current_state_with_user_id(
        mpv: &mut MpvController,
        playlist_files: &[PathBuf],
        user_id: &str,
    ) -> Result<UserState> {
        // Add longer delays between requests to give MPV time to respond properly
        let playlist_pos = mpv.get_playlist_pos().await.unwrap_or(0);
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        
        let playback_time = mpv.get_position().await.unwrap_or(0.0);
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        
        let is_paused = mpv.is_paused().await.unwrap_or(true);
        
        let current_file = if playlist_pos >= 0 && (playlist_pos as usize) < playlist_files.len() {
            Some(playlist_files[playlist_pos as usize].clone())
        } else {
            None
        };
        
        // Debug logging to help diagnose position issues
        if let Some(ref file) = current_file {
            if let Some(filename) = file.file_name().and_then(|n| n.to_str()) {
                debug!("User {}: MPV pos={}, file={}, total_files={}", 
                      user_id, playlist_pos, filename, playlist_files.len());
            }
        }
        
        let mut state = UserState::new(user_id.to_string());
        state.update_from_mpv(playlist_pos, playback_time, is_paused, current_file);
        
        Ok(state)
    }
    
    /// Static version for use in spawned tasks (deprecated - use get_current_state_with_user_id)
    async fn get_current_state_static(
        mpv: &mut MpvController,
        playlist_files: &[PathBuf],
    ) -> Result<UserState> {
        let playlist_pos = mpv.get_playlist_pos().await.unwrap_or(0);
        let playback_time = mpv.get_position().await.unwrap_or(0.0);
        let is_paused = mpv.is_paused().await.unwrap_or(true);
        
        let current_file = if playlist_pos >= 0 && (playlist_pos as usize) < playlist_files.len() {
            Some(playlist_files[playlist_pos as usize].clone())
        } else {
            None
        };
        
        let mut state = UserState::new("temp".to_string()); // Will be overwritten
        state.update_from_mpv(playlist_pos, playback_time, is_paused, current_file);
        
        Ok(state)
    }
    
    /// Handle incoming message from server
    async fn handle_incoming_message(&self, message: SyncMessage) {
        match message.event {
            SyncEvent::UserJoined { user_id, user_state } => {
                if user_id != self.user_id {
                    info!("User {} joined: {}", user_id, user_state.format_for_display());
                }
            }
            
            SyncEvent::UserLeft { user_id } => {
                if user_id != self.user_id {
                    info!("User {} left the session", user_id);
                }
            }
            
            SyncEvent::StateUpdate { user_state } => {
                if user_state.user_id != self.user_id {
                    debug!("User state: {}", user_state.format_for_display());
                }
            }
            
            SyncEvent::Heartbeat { user_id, .. } => {
                debug!("Heartbeat from {}", user_id);
            }
            
            SyncEvent::UserAction { user_id, action, value } => {
                info!("User {} performed action: {} {:?}", user_id, action, value);
            }
        }
    }
    
    /// Send a message to the server
    async fn send_message(&self, writer: &mut tokio::net::tcp::OwnedWriteHalf, message: SyncMessage) -> Result<()> {
        let json = serde_json::to_string(&message)?;
        writer.write_all(format!("{}\n", json).as_bytes()).await?;
        writer.flush().await?;
        Ok(())
    }
    
    /// Static version for use in spawned tasks
    async fn send_message_static(writer: &mut tokio::net::tcp::OwnedWriteHalf, message: SyncMessage) -> Result<()> {
        let json = serde_json::to_string(&message)?;
        writer.write_all(format!("{}\n", json).as_bytes()).await?;
        writer.flush().await?;
        Ok(())
    }
    
    /// Get next sequence number
    fn next_sequence(&mut self) -> u64 {
        self.sequence_counter += 1;
        self.sequence_counter
    }
}
