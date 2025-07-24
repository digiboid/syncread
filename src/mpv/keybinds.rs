use std::path::{Path, PathBuf};
use std::fs;
use anyhow::{Result, Context};
use tracing::info;

pub struct KeybindProfile {
    keybinds: Vec<(String, String)>, // (key, command)
}

impl KeybindProfile {
    /// Create a new keybind profile optimized for synchronized media viewing
    pub fn new_sync_profile() -> Self {
        let mut keybinds = Vec::new();
        
        // Basic playback controls
        keybinds.push(("SPACE".to_string(), "cycle pause".to_string()));
        keybinds.push(("p".to_string(), "cycle pause".to_string()));
        
        // Navigation - LEFT/RIGHT for prev/next file
        keybinds.push(("LEFT".to_string(), "playlist-prev".to_string()));
        keybinds.push(("RIGHT".to_string(), "playlist-next".to_string()));
        
        // Seeking with other keys
        keybinds.push(("DOWN".to_string(), "seek -30".to_string()));
        keybinds.push(("UP".to_string(), "seek 30".to_string()));
        keybinds.push(("Shift+LEFT".to_string(), "seek -5".to_string()));
        keybinds.push(("Shift+RIGHT".to_string(), "seek 5".to_string()));
        
        // Playlist navigation
        keybinds.push(("n".to_string(), "playlist-next".to_string()));
        keybinds.push(("N".to_string(), "playlist-prev".to_string()));
        keybinds.push((">".to_string(), "playlist-next".to_string()));
        keybinds.push(("<".to_string(), "playlist-prev".to_string()));
        
        // Picture/media specific controls
        keybinds.push(("z".to_string(), "add video-zoom 0.1".to_string()));
        keybinds.push(("Z".to_string(), "add video-zoom -0.1".to_string()));
        keybinds.push(("r".to_string(), "set video-zoom 0; set video-pan-x 0; set video-pan-y 0".to_string()));
        
        // Pan controls for images
        keybinds.push(("h".to_string(), "add video-pan-x -0.05".to_string()));
        keybinds.push(("l".to_string(), "add video-pan-x 0.05".to_string()));
        keybinds.push(("k".to_string(), "add video-pan-y -0.05".to_string()));
        keybinds.push(("j".to_string(), "add video-pan-y 0.05".to_string()));
        
        // Rotation
        keybinds.push(("Ctrl+LEFT".to_string(), "add video-rotate -90".to_string()));
        keybinds.push(("Ctrl+RIGHT".to_string(), "add video-rotate 90".to_string()));
        
        // Speed controls
        keybinds.push(("=".to_string(), "add speed 0.1".to_string()));
        keybinds.push(("-".to_string(), "add speed -0.1".to_string()));
        keybinds.push(("BS".to_string(), "set speed 1.0".to_string()));
        
        // Volume
        keybinds.push(("9".to_string(), "add volume -5".to_string()));
        keybinds.push(("0".to_string(), "add volume 5".to_string()));
        keybinds.push(("m".to_string(), "cycle mute".to_string()));
        
        // Fullscreen and window controls
        keybinds.push(("f".to_string(), "cycle fullscreen".to_string()));
        keybinds.push(("ESC".to_string(), "set fullscreen no".to_string()));
        
        // Info display
        keybinds.push(("i".to_string(), "script-binding stats/display-stats-toggle".to_string()));
        keybinds.push(("I".to_string(), "script-binding stats/display-page-4".to_string()));
        
        // Quit
        keybinds.push(("q".to_string(), "quit".to_string()));
        keybinds.push(("Q".to_string(), "quit-watch-later".to_string()));
        
        // Disable some default keys that might interfere with sync
        keybinds.push(("s".to_string(), "ignore".to_string())); // Disable screenshot
        keybinds.push(("S".to_string(), "ignore".to_string())); // Disable screenshot
        
        Self { keybinds }
    }
    
    /// Add a custom keybind
    pub fn add_keybind(&mut self, key: String, command: String) {
        self.keybinds.push((key, command));
    }
    
    /// Remove keybind for a specific key
    pub fn remove_keybind(&mut self, key: &str) {
        self.keybinds.retain(|(k, _)| k != key);
    }
    
    /// Generate the keybind config file content
    pub fn generate_config(&self) -> String {
        let mut config = String::new();
        
        config.push_str("# SyncRead MPV Keybind Profile\n");
        config.push_str("# Generated automatically - do not edit manually\n\n");
        
        for (key, command) in &self.keybinds {
            config.push_str(&format!("{:<20} {}\n", key, command));
        }
        
        config
    }
    
    /// Write keybind config to file
    pub fn write_to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let config_content = self.generate_config();
        
        fs::write(&path, config_content)
            .with_context(|| format!("Failed to write keybind config to {:?}", path.as_ref()))?;
        
        info!("Keybind profile written to: {:?}", path.as_ref());
        Ok(())
    }
    
    /// Create a temporary keybind config file
    pub fn create_temp_config(&self) -> Result<PathBuf> {
        let temp_dir = std::env::temp_dir();
        let config_path = temp_dir.join("syncread_keybinds.conf");
        
        self.write_to_file(&config_path)?;
        
        Ok(config_path)
    }
}

impl Default for KeybindProfile {
    fn default() -> Self {
        Self::new_sync_profile()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_keybind_profile_creation() {
        let profile = KeybindProfile::new_sync_profile();
        let config = profile.generate_config();
        
        assert!(config.contains("SPACE"));
        assert!(config.contains("cycle pause"));
        assert!(config.contains("playlist-next"));
    }
    
    #[test]
    fn test_add_custom_keybind() {
        let mut profile = KeybindProfile::new_sync_profile();
        profile.add_keybind("x".to_string(), "show-text hello".to_string());
        
        let config = profile.generate_config();
        assert!(config.contains("x"));
        assert!(config.contains("show-text hello"));
    }
}
