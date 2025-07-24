use super::protocol::{SyncMessage, SyncEvent, UserId, UserState, SessionState};
use crate::mpv::MpvController;
use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::net::TcpStream;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, RwLock};
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, warn};
use std::sync::Arc;

/// Client that connects to sync server and synchronizes MPV state
pub struct SyncClient {
    user_id: UserId,
    sequence_counter: u64,
    session_state: Arc<RwLock<SessionState>>,
    last_known_position: Arc<RwLock<Option<i32>>>,
    pending_position: Arc<RwLock<Option<(i32, u8)>>>, // (position, retry_count)
}

impl SyncClient {
    /// Create a new sync client
    pub fn new(user_id: UserId) -> Self {
        Self {
            user_id,
            sequence_counter: 0,
            session_state: Arc::new(RwLock::new(SessionState::new())),
            last_known_position: Arc::new(RwLock::new(None)),
            pending_position: Arc::new(RwLock::new(None)),
        }
    }
    
    /// Connect to sync server and start synchronization
    pub async fn connect_and_sync(
        &mut self,
        server_addr: SocketAddr,
        mut mpv_controller: MpvController,
        playlist_files: Vec<PathBuf>,
        minimal: bool,
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
        
        // Add our own state to the session and set initial position
        self.session_state.write().await.update_user(initial_state.clone());
        *self.last_known_position.write().await = Some(initial_state.playlist_position);
        
        // Start the display loop
        let session_state_for_display = self.session_state.clone();
        let user_id_for_display = self.user_id.clone();
        tokio::spawn(async move {
            Self::display_loop(session_state_for_display, user_id_for_display, minimal).await;
        });
        
        // Start periodic state updates
        let outgoing_tx_clone = outgoing_tx.clone();
        let user_id_clone = self.user_id.clone();
        let session_state_for_updates = self.session_state.clone();
        let last_known_position_clone = self.last_known_position.clone();
        let pending_position_clone = self.pending_position.clone();
        let mut sequence_counter = self.sequence_counter;
        
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_millis(1000)); // Update every second
            
            loop {
                interval.tick().await;
                
                match Self::get_current_state_with_user_id(&mut mpv_controller, &playlist_files, &user_id_clone).await {
                    Ok(state) => {
                        // Validate position change to prevent MPV transition glitches
                        let should_send_update = Self::validate_position_change(
                            &last_known_position_clone,
                            &pending_position_clone,
                            state.playlist_position,
                            playlist_files.len()
                        ).await;
                        
                        if should_send_update {
                            // Update our local session state
                            session_state_for_updates.write().await.update_user(state.clone());
                            
                            sequence_counter += 1;
                            let update_message = SyncMessage::state_update(state, sequence_counter);
                            
                            if let Err(e) = outgoing_tx_clone.send(update_message) {
                                error!("Failed to send state update: {}", e);
                                break;
                            }
                        } else {
                            debug!("Skipped potentially invalid position update: {}", state.playlist_position);
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
                debug!("🔍 User {}: MPV reports pos={}, file={}, total_files={}", 
                      user_id, playlist_pos, filename, playlist_files.len());
                debug!("   📤 Sending to server: pos={}, file={}", 
                       playlist_pos, filename);
            }
        } else {
            debug!("🔍 User {}: MPV reports pos={}, file=None, total_files={}", 
                  user_id, playlist_pos, playlist_files.len());
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
            SyncEvent::UserJoined { user_id: _, user_state } => {
                self.session_state.write().await.update_user(user_state);
            }
            
            SyncEvent::UserLeft { user_id } => {
                self.session_state.write().await.remove_user(&user_id);
            }
            
            SyncEvent::StateUpdate { user_state } => {
                self.session_state.write().await.update_user(user_state);
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
    
    /// Display loop showing current session state for client
    async fn display_loop(session_state: Arc<RwLock<SessionState>>, current_user_id: UserId, minimal: bool) {
        use tokio::time::{interval, Duration};

        let mut interval = interval(Duration::from_millis(1000)); // Update every second

        loop {
            interval.tick().await;

            let state = session_state.read().await;
            let relative_info = Self::get_relative_position_info(&state, &current_user_id);

            // ANSI escape code to clear screen and move cursor to top-left
            print!("\x1b[2J\x1b[1;1H");

            if !state.users.is_empty() {
                if minimal {
                    // Minimal mode: only show relative position info
                    println!("🎬 SyncRead Client ({}) - Minimal Mode", current_user_id);
                    println!("{}", "=".repeat(40));
                    if !relative_info.is_empty() {
                        println!("{}", relative_info);
                    } else {
                        println!("📍 You are the only user connected");
                    }
                    println!("{}", "=".repeat(40));
                } else {
                    // Full mode: show all users and relative info
                    let user_count = state.users.len();
                    let display_lines = state.format_for_display();
                    println!("🎬 SyncRead Client ({}) - {} users connected", current_user_id, user_count);
                    println!("{}", "=".repeat(60));

                    for line in display_lines {
                        let is_current_user = line.starts_with(&format!("{}:", current_user_id));
                        if is_current_user {
                            println!("👤 {}", line);
                        } else {
                            println!("   {}", line);
                        }
                    }

                    println!("{}", "=".repeat(60));
                    if !relative_info.is_empty() {
                        println!("{}", relative_info);
                    }
                }
                println!("Press 'q' in MPV to quit, or Ctrl+C here");
            }
        }
    }
    
    /// Get relative position information compared to other users
    fn get_relative_position_info(session_state: &SessionState, current_user_id: &UserId) -> String {
        if session_state.users.len() <= 1 {
            return String::new();
        }
        
        let current_user = match session_state.users.get(current_user_id) {
            Some(user) => user,
            None => return String::new(),
        };
        
        let current_pos = current_user.playlist_position;
        let other_users: Vec<_> = session_state.users.values()
            .filter(|u| u.user_id != *current_user_id)
            .collect();
            
        if other_users.is_empty() {
            return String::new();
        }
        
        // Calculate relative positions
        let mut same_page = Vec::new();
        let mut ahead_of = Vec::new();
        let mut behind = Vec::new();
        
        for user in other_users {
            let diff = current_pos - user.playlist_position;
            if diff == 0 {
                same_page.push(&user.user_id);
            } else if diff > 0 {
                ahead_of.push((&user.user_id, diff));
            } else {
                behind.push((&user.user_id, -diff));
            }
        }
        
        let mut messages = Vec::new();
        
        if !same_page.is_empty() {
            if same_page.len() == 1 {
                messages.push(format!("📍 You are on the same page as {}", same_page[0]));
            } else {
                let names: Vec<String> = same_page.iter().map(|s| s.to_string()).collect();
                messages.push(format!("📍 You are on the same page as {}", names.join(", ")));
            }
        }
        
        for (user_id, pages) in ahead_of {
            let page_word = if pages == 1 { "page" } else { "pages" };
            messages.push(format!("⬆️  You are {} {} ahead of {}", pages, page_word, user_id));
        }
        
        for (user_id, pages) in behind {
            let page_word = if pages == 1 { "page" } else { "pages" };
            messages.push(format!("⬇️  You are {} {} behind {}", pages, page_word, user_id));
        }
        
        messages.join("\n")
    }
    
    /// Validate position change to prevent MPV transition glitches with retry mechanism
    async fn validate_position_change(
        last_known_position: &Arc<RwLock<Option<i32>>>,
        pending_position: &Arc<RwLock<Option<(i32, u8)>>>,
        new_position: i32,
        playlist_length: usize
    ) -> bool {
        let mut last_pos = last_known_position.write().await;
        let mut pending = pending_position.write().await;
        
        // If we don't have a last known position, accept any reasonable position
        let Some(last) = *last_pos else {
            if new_position >= 0 && new_position < playlist_length as i32 {
                *last_pos = Some(new_position);
                *pending = None; // Clear any pending position
                return true;
            }
            return false;
        };
        
        // Reject jumping to invalid positions
        if new_position < 0 || new_position >= playlist_length as i32 {
            debug!("Rejected invalid position: {} (playlist length: {})", new_position, playlist_length);
            *pending = None; // Clear pending for invalid positions
            return false;
        }
        
        let position_diff = (new_position - last).abs();
        
        // Always allow small jumps (±3 positions)
        if position_diff <= 3 {
            *last_pos = Some(new_position);
            *pending = None; // Clear any pending position
            return true;
        }
        
        // Allow larger jumps if they seem intentional (forward progress)
        if new_position > last {
            *last_pos = Some(new_position);
            *pending = None; // Clear any pending position
            return true;
        }
        
        // Handle large backward jumps with retry mechanism
        if position_diff > 10 {
            // Check if this is a glitch (jumping from middle/end back to start)
            if last > 5 && new_position <= 1 {
                debug!("Rejected obvious glitch: position {} -> {} (likely MPV transition)", last, new_position);
                *pending = None; // Clear pending for obvious glitches
                return false;
            }
            
            // Handle legitimate large backward jumps with retry
            match pending.as_mut() {
                Some((pending_pos, count)) if *pending_pos == new_position => {
                    // Same position persists, increment retry count
                    *count += 1;
                    debug!("Large backward jump {} -> {} persists (attempt {})", last, new_position, *count);
                    
                    // Accept after 2 consistent readings (2 seconds total)
                    if *count >= 2 {
                        *last_pos = Some(new_position);
                        *pending = None;
                        info!("Accepted legitimate large backward jump: {} -> {}", last, new_position);
                        return true;
                    }
                }
                _ => {
                    // New large backward jump, start tracking it
                    *pending = Some((new_position, 1));
                    debug!("Tracking potential large backward jump: {} -> {} (attempt 1)", last, new_position);
                }
            }
            return false; // Don't send update yet, wait for confirmation
        }
        
        // Accept moderate backward jumps (user might have gone back a few pages)
        *last_pos = Some(new_position);
        *pending = None; // Clear any pending position
        true
    }
}
