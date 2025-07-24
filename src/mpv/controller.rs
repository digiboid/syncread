use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use tokio::net::UnixStream;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use serde::{Deserialize, Serialize};
use anyhow::{Result, Context};
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MpvCommand {
    pub command: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MpvResponse {
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
    #[serde(default)]
    pub request_id: Option<u32>,
}

pub struct MpvController {
    process: Child,
    socket_path: PathBuf,
    connection: Option<UnixStream>,
    next_request_id: u32,
}

impl MpvController {
    /// Launch MPV with IPC socket and keybind profile
    pub async fn launch<P: AsRef<Path>>(
        socket_path: P,
        keybind_config: Option<P>,
        media_files: Vec<P>
    ) -> Result<Self> {
        let socket_path = socket_path.as_ref().to_path_buf();
        
        // Build MPV command
        let mut cmd = Command::new("mpv");
        
        // Essential IPC setup
        cmd.arg(format!("--input-ipc-server={}", socket_path.display()))
           .arg("--idle=yes")
           .arg("--force-window=yes");
        
        // Add keybind config if provided
        if let Some(config_path) = keybind_config {
            cmd.arg(format!("--input-conf={}", config_path.as_ref().display()));
        }
        
        // Add media files
        for file in media_files {
            cmd.arg(file.as_ref());
        }
        
        // Show stdout/stderr for debugging
        cmd.stdout(Stdio::inherit())
           .stderr(Stdio::inherit());
        
        info!("Launching MPV with socket: {:?}", socket_path);
        
        let process = cmd.spawn()
            .context("Failed to spawn MPV process")?;
        
        let mut controller = Self {
            process,
            socket_path,
            connection: None,
            next_request_id: 1,
        };
        
        // Wait for socket to be ready
        controller.wait_for_socket().await?;
        
        Ok(controller)
    }
    
    /// Wait for MPV to create the IPC socket
    async fn wait_for_socket(&mut self) -> Result<()> {
        use tokio::time::{sleep, Duration};
        
        info!("Waiting for MPV socket at: {:?}", self.socket_path);
        
        for attempt in 1..=50 { // 5 second timeout
            if self.socket_path.exists() {
                debug!("MPV socket ready after {} attempts", attempt);
                return Ok(());
            }
            
            if attempt % 10 == 0 {
                info!("Still waiting for socket... attempt {}/50", attempt);
            }
            
            sleep(Duration::from_millis(100)).await;
        }
        
        error!("Socket file does not exist after timeout: {:?}", self.socket_path);
        
        // Check if MPV process is still running
        match self.process.try_wait() {
            Ok(Some(status)) => error!("MPV process exited with status: {:?}", status),
            Ok(None) => error!("MPV process is still running but no socket created"),
            Err(e) => error!("Failed to check MPV process status: {}", e),
        }
        
        anyhow::bail!("MPV socket not created within timeout");
    }
    
    /// Connect to MPV IPC socket
    pub async fn connect(&mut self) -> Result<()> {
        if self.connection.is_some() {
            return Ok(()); // Already connected
        }
        
        let stream = UnixStream::connect(&self.socket_path).await
            .context("Failed to connect to MPV socket")?;
        
        self.connection = Some(stream);
        info!("Connected to MPV IPC socket");
        
        Ok(())
    }
    
    /// Send command to MPV and get response
    pub async fn send_command(&mut self, command: Vec<serde_json::Value>) -> Result<MpvResponse> {
        self.connect().await?;
        
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        
        let mpv_command = MpvCommand {
            command,
            request_id: Some(request_id),
        };
        
        let json_command = serde_json::to_string(&mpv_command)?;
        debug!("Sending MPV command: {}", json_command);
        
        // Send command
        if let Some(ref mut connection) = self.connection {
            connection.write_all(json_command.as_bytes()).await?;
            connection.write_all(b"\n").await?;
            connection.flush().await?;
            
            // Read response - retry on empty/invalid responses
            let mut reader = BufReader::new(connection);
            
            for attempt in 1..=10 {
                let mut response_line = String::new();
                reader.read_line(&mut response_line).await?;
                
                let trimmed = response_line.trim();
                if trimmed.is_empty() {
                    debug!("Empty response, attempt {}/10", attempt);
                    continue;
                }
                
                match serde_json::from_str::<MpvResponse>(trimmed) {
                    Ok(response) => {
                        debug!("MPV response: {:?}", response);
                        
                        // Only accept responses that match our exact request_id
                        if response.request_id == Some(request_id) {
                            if response.error != "success" && !response.error.is_empty() {
                                warn!("MPV command error: {}", response.error);
                            }
                            return Ok(response);
                        } else {
                            debug!("Skipping response for different request: expected {}, got {:?}", 
                                   request_id, response.request_id);
                            continue;
                        }
                    }
                    Err(e) => {
                        warn!("Failed to parse response '{}': {}", trimmed, e);
                        continue;
                    }
                }
            }
            
            anyhow::bail!("Failed to get valid response from MPV");
        } else {
            anyhow::bail!("Not connected to MPV");
        }
    }
    
    /// Convenience methods for common MPV commands
    
    pub async fn play(&mut self) -> Result<()> {
        self.send_command(vec!["set_property".into(), "pause".into(), false.into()]).await?;
        Ok(())
    }
    
    pub async fn pause(&mut self) -> Result<()> {
        self.send_command(vec!["set_property".into(), "pause".into(), true.into()]).await?;
        Ok(())
    }
    
    pub async fn seek(&mut self, seconds: f64) -> Result<()> {
        self.send_command(vec!["seek".into(), seconds.into()]).await?;
        Ok(())
    }
    
    pub async fn next_file(&mut self) -> Result<()> {
        self.send_command(vec!["playlist-next".into()]).await?;
        Ok(())
    }
    
    pub async fn prev_file(&mut self) -> Result<()> {
        self.send_command(vec!["playlist-prev".into()]).await?;
        Ok(())
    }
    
    pub async fn get_position(&mut self) -> Result<f64> {
        let response = self.send_command(vec!["get_property".into(), "playback-time".into()]).await?;
        
        if let Some(data) = response.data {
            if let Some(pos) = data.as_f64() {
                return Ok(pos);
            }
        }
        
        Ok(0.0) // Default if not available
    }
    
    pub async fn get_playlist_pos(&mut self) -> Result<i32> {
        let response = self.send_command(vec!["get_property".into(), "playlist-pos".into()]).await?;
        
        if let Some(data) = response.data {
            if let Some(pos) = data.as_i64() {
                return Ok(pos as i32);
            }
        }
        
        Ok(0)
    }
    
    pub async fn is_paused(&mut self) -> Result<bool> {
        let response = self.send_command(vec!["get_property".into(), "pause".into()]).await?;
        
        if let Some(data) = response.data {
            if let Some(paused) = data.as_bool() {
                return Ok(paused);
            }
        }
        
        Ok(true) // Default to paused if unknown
    }
}

impl Drop for MpvController {
    fn drop(&mut self) {
        // Terminate MPV process when controller is dropped
        if let Err(e) = self.process.kill() {
            error!("Failed to kill MPV process: {}", e);
        }
        
        // Clean up socket file
        if self.socket_path.exists() {
            if let Err(e) = std::fs::remove_file(&self.socket_path) {
                warn!("Failed to remove socket file: {}", e);
            }
        }
    }
}
