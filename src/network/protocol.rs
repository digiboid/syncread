use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Unique identifier for users in the sync session
pub type UserId = String;

/// Current state of a user's media playback
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserState {
    pub user_id: UserId,
    pub playlist_position: i32,
    pub current_file: Option<PathBuf>,
    pub current_file_name: Option<String>, // For display purposes
    pub playback_time: f64,
    pub is_paused: bool,
    pub timestamp: u64, // Unix timestamp when this state was created
}

impl UserState {
    pub fn new(user_id: UserId) -> Self {
        Self {
            user_id,
            playlist_position: 0,
            current_file: None,
            current_file_name: None,
            playback_time: 0.0,
            is_paused: true,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }
    
    /// Update state from MPV controller data
    pub fn update_from_mpv(
        &mut self, 
        playlist_pos: i32, 
        playback_time: f64, 
        is_paused: bool,
        current_file: Option<PathBuf>,
    ) {
        self.playlist_position = playlist_pos;
        self.playback_time = playback_time;
        self.is_paused = is_paused;
        
        if let Some(ref file) = current_file {
            self.current_file_name = file.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string());
        }
        
        self.current_file = current_file;
        self.timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
    }
    
    /// Format for CLI display
    pub fn format_for_display(&self) -> String {
        let file_name = self.current_file_name
            .as_deref()
            .unwrap_or("(no file)");
            
        let status = if self.is_paused { "⏸" } else { "▶" };
        
        format!("{}: {} {} (pos: {}, time: {:.1}s)", 
                self.user_id, 
                status,
                file_name, 
                self.playlist_position,
                self.playback_time)
    }
}

/// Events that can be synchronized between users
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncEvent {
    /// User joined the session
    UserJoined {
        user_id: UserId,
        user_state: UserState,
    },
    
    /// User left the session
    UserLeft {
        user_id: UserId,
    },
    
    /// User updated their playback state
    StateUpdate {
        user_state: UserState,
    },
    
    /// User performed an action (play, pause, seek, etc.)
    UserAction {
        user_id: UserId,
        action: String,
        value: Option<f64>,
    },
    
    /// Heartbeat to keep connection alive
    Heartbeat {
        user_id: UserId,
        timestamp: u64,
    },
}

/// Messages sent over the network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncMessage {
    pub event: SyncEvent,
    pub sequence: u64, // For ordering messages
}

impl SyncMessage {
    pub fn new(event: SyncEvent, sequence: u64) -> Self {
        Self { event, sequence }
    }
    
    /// Create a state update message
    pub fn state_update(user_state: UserState, sequence: u64) -> Self {
        Self::new(SyncEvent::StateUpdate { user_state }, sequence)
    }
    
    /// Create a user joined message
    pub fn user_joined(user_id: UserId, user_state: UserState, sequence: u64) -> Self {
        Self::new(SyncEvent::UserJoined { user_id, user_state }, sequence)
    }
    
    /// Create a user left message
    pub fn user_left(user_id: UserId, sequence: u64) -> Self {
        Self::new(SyncEvent::UserLeft { user_id }, sequence)
    }
    
    /// Create a heartbeat message
    pub fn heartbeat(user_id: UserId, sequence: u64) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
            
        Self::new(SyncEvent::Heartbeat { user_id, timestamp }, sequence)
    }
}

/// Manages the state of all users in a sync session
#[derive(Debug, Clone)]
pub struct SessionState {
    pub users: HashMap<UserId, UserState>,
    pub created_at: u64,
}

impl SessionState {
    pub fn new() -> Self {
        Self {
            users: HashMap::new(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }
    
    /// Add or update a user's state
    pub fn update_user(&mut self, user_state: UserState) {
        self.users.insert(user_state.user_id.clone(), user_state);
    }
    
    /// Remove a user from the session
    pub fn remove_user(&mut self, user_id: &UserId) {
        self.users.remove(user_id);
    }
    
    /// Get all users sorted by user ID for consistent display
    pub fn get_users_sorted(&self) -> Vec<&UserState> {
        let mut users: Vec<&UserState> = self.users.values().collect();
        users.sort_by(|a, b| a.user_id.cmp(&b.user_id));
        users
    }
    
    /// Format all users for CLI display
    pub fn format_for_display(&self) -> Vec<String> {
        self.get_users_sorted()
            .into_iter()
            .map(|user| user.format_for_display())
            .collect()
    }
    
    /// Check if users are synchronized (within tolerance)
    pub fn check_sync_status(&self, position_tolerance: i32) -> bool {
        if self.users.len() < 2 {
            return true; // Single user is always "in sync"
        }
        
        let positions: Vec<i32> = self.users.values()
            .map(|u| u.playlist_position)
            .collect();
            
        let min_pos = positions.iter().min().unwrap_or(&0);
        let max_pos = positions.iter().max().unwrap_or(&0);
        
        (max_pos - min_pos) <= position_tolerance
    }
    
    /// Get sync status summary
    pub fn get_sync_summary(&self) -> String {
        let user_count = self.users.len();
        let in_sync = self.check_sync_status(1); // Allow 1 position difference
        
        let status = if in_sync { "✅ In Sync" } else { "⚠️ Out of Sync" };
        
        format!("{} users connected - {}", user_count, status)
    }
}

impl Default for SessionState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_user_state_creation() {
        let state = UserState::new("user1".to_string());
        assert_eq!(state.user_id, "user1");
        assert_eq!(state.playlist_position, 0);
        assert!(state.is_paused);
    }
    
    #[test]
    fn test_session_state_sync_check() {
        let mut session = SessionState::new();
        
        let mut user1 = UserState::new("user1".to_string());
        user1.playlist_position = 5;
        
        let mut user2 = UserState::new("user2".to_string());
        user2.playlist_position = 5;
        
        session.update_user(user1);
        session.update_user(user2);
        
        assert!(session.check_sync_status(1));
        
        // Make them out of sync
        let mut user2_updated = session.users["user2"].clone();
        user2_updated.playlist_position = 10;
        session.update_user(user2_updated);
        
        assert!(!session.check_sync_status(1));
    }
}
