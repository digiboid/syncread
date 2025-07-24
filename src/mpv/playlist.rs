use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlaylistItem {
    pub path: PathBuf,
    pub title: Option<String>,
    pub duration: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistState {
    pub items: Vec<PlaylistItem>,
    pub current_index: i32,
    pub current_position: f64, // seconds
    pub is_paused: bool,
}

impl PlaylistState {
    /// Create a new playlist state from media files
    pub fn new(files: Vec<PathBuf>) -> Self {
        let items = files.into_iter()
            .map(|path| PlaylistItem {
                title: path.file_name()
                    .and_then(|name| name.to_str())
                    .map(|s| s.to_string()),
                path,
                duration: None, // Will be filled when MPV loads the file
            })
            .collect();
        
        Self {
            items,
            current_index: 0,
            current_position: 0.0,
            is_paused: true,
        }
    }
    
    /// Get the currently playing item
    pub fn current_item(&self) -> Option<&PlaylistItem> {
        if self.current_index >= 0 && (self.current_index as usize) < self.items.len() {
            Some(&self.items[self.current_index as usize])
        } else {
            None
        }
    }
    
    /// Update the current playlist position
    pub fn update_position(&mut self, index: i32, position: f64, paused: bool) -> bool {
        let changed = self.current_index != index 
            || (self.current_position - position).abs() > 0.5 // Only update if >0.5s difference
            || self.is_paused != paused;
            
        self.current_index = index;
        self.current_position = position;
        self.is_paused = paused;
        
        changed
    }
    
    /// Move to next item in playlist
    pub fn next(&mut self) -> bool {
        if (self.current_index as usize) < self.items.len().saturating_sub(1) {
            self.current_index += 1;
            self.current_position = 0.0;
            true
        } else {
            false
        }
    }
    
    /// Move to previous item in playlist
    pub fn prev(&mut self) -> bool {
        if self.current_index > 0 {
            self.current_index -= 1;
            self.current_position = 0.0;
            true
        } else {
            false
        }
    }
    
    /// Check if playlist is at the end
    pub fn is_at_end(&self) -> bool {
        self.current_index >= self.items.len() as i32 - 1
    }
    
    /// Check if playlist is at the beginning
    pub fn is_at_beginning(&self) -> bool {
        self.current_index <= 0
    }
    
    /// Get total number of items
    pub fn len(&self) -> usize {
        self.items.len()
    }
    
    /// Check if playlist is empty
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    
    /// Get progress through current item (0.0 to 1.0)
    pub fn current_progress(&self) -> f64 {
        if let Some(item) = self.current_item() {
            if let Some(duration) = item.duration {
                if duration > 0.0 {
                    return (self.current_position / duration).clamp(0.0, 1.0);
                }
            }
        }
        0.0
    }
    
    /// Get formatted time string for current position
    pub fn format_current_time(&self) -> String {
        format_time(self.current_position)
    }
    
    /// Get formatted duration string for current item
    pub fn format_current_duration(&self) -> String {
        self.current_item()
            .and_then(|item| item.duration)
            .map(format_time)
            .unwrap_or_else(|| "--:--".to_string())
    }
    
    /// Update duration for current item
    pub fn update_current_duration(&mut self, duration: f64) {
        if let Some(item) = self.current_item_mut() {
            item.duration = Some(duration);
        }
    }
    
    fn current_item_mut(&mut self) -> Option<&mut PlaylistItem> {
        if self.current_index >= 0 && (self.current_index as usize) < self.items.len() {
            Some(&mut self.items[self.current_index as usize])
        } else {
            None
        }
    }
}

impl Default for PlaylistState {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

/// Format seconds as MM:SS or HH:MM:SS
fn format_time(seconds: f64) -> String {
    let total_seconds = seconds as u64;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let secs = total_seconds % 60;
    
    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, secs)
    } else {
        format!("{:02}:{:02}", minutes, secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_playlist_creation() {
        let files = vec![
            PathBuf::from("/path/to/file1.mp4"),
            PathBuf::from("/path/to/file2.jpg"),
        ];
        
        let playlist = PlaylistState::new(files);
        
        assert_eq!(playlist.len(), 2);
        assert_eq!(playlist.current_index, 0);
        assert_eq!(playlist.current_position, 0.0);
        assert!(playlist.is_paused);
    }
    
    #[test]
    fn test_navigation() {
        let files = vec![
            PathBuf::from("/path/to/file1.mp4"),
            PathBuf::from("/path/to/file2.jpg"),
            PathBuf::from("/path/to/file3.png"),
        ];
        
        let mut playlist = PlaylistState::new(files);
        
        // Test next
        assert!(playlist.next());
        assert_eq!(playlist.current_index, 1);
        
        assert!(playlist.next());
        assert_eq!(playlist.current_index, 2);
        
        // At end, next should return false
        assert!(!playlist.next());
        assert_eq!(playlist.current_index, 2);
        
        // Test prev
        assert!(playlist.prev());
        assert_eq!(playlist.current_index, 1);
        
        assert!(playlist.prev());
        assert_eq!(playlist.current_index, 0);
        
        // At beginning, prev should return false
        assert!(!playlist.prev());
        assert_eq!(playlist.current_index, 0);
    }
    
    #[test]
    fn test_time_formatting() {
        assert_eq!(format_time(65.0), "01:05");
        assert_eq!(format_time(3665.0), "01:01:05");
        assert_eq!(format_time(30.5), "00:30");
    }
    
    #[test]
    fn test_position_update() {
        let files = vec![PathBuf::from("/test.mp4")];
        let mut playlist = PlaylistState::new(files);
        
        // Small changes shouldn't trigger update
        assert!(!playlist.update_position(0, 0.3, true));
        
        // Large changes should trigger update
        assert!(playlist.update_position(0, 1.0, true));
        
        // State changes should trigger update
        assert!(playlist.update_position(0, 1.0, false));
    }
}
