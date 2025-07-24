mod mpv;
mod network;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::{debug, info, Level};
use tracing_subscriber;

use mpv::{KeybindProfile, MpvController};
use network::{SyncClient, SyncServer};

#[derive(Parser)]
#[command(name = "syncread")]
#[command(about = "Synchronized media viewer using MPV")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a sync server (host mode)
    Server {
        /// Address to bind the server to
        #[arg(short, long, default_value = "127.0.0.1:8080")]
        bind: SocketAddr,
    },
    /// Connect to a sync server (client mode)
    Client {
        /// Server address to connect to
        #[arg(short, long, default_value = "127.0.0.1:8080")]
        server: SocketAddr,
        /// User ID for this client
        #[arg(short, long)]
        user_id: String,
        /// Media files or directory to load
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Test MPV controller only (no networking)
    Test {
        /// Media files to test with
        files: Vec<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG) // Enable debug for troubleshooting
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Server { bind } => {
            info!("🚀 Starting SyncRead server mode");
            start_server(bind).await
        }
        Commands::Client { server, user_id, files } => {
            info!("🔗 Starting SyncRead client mode");
            start_client(server, user_id, files).await
        }
        Commands::Test { files } => {
            info!("🧪 Testing MPV controller");
            test_mpv_controller(files).await
        }
    }
}

async fn start_server(bind_addr: SocketAddr) -> Result<()> {
    let server = SyncServer::new();
    info!("Starting sync server on {}", bind_addr);
    info!("Clients can connect with: syncread client --server {} --user-id <name> <files...>", bind_addr);
    
    server.start(bind_addr).await?;
    Ok(())
}

async fn start_client(server_addr: SocketAddr, user_id: String, files: Vec<PathBuf>) -> Result<()> {
    info!("Connecting to server {} as user '{}'", server_addr, user_id);
    
    // Expand directories and validate files
    let media_files = expand_media_files(files)?;
    if media_files.is_empty() {
        anyhow::bail!("No media files found");
    }
    
    info!("Loaded {} media files", media_files.len());
    
    // Debug: show first few files for troubleshooting
    debug!("First few files loaded:");
    for (i, file) in media_files.iter().take(5).enumerate() {
        if let Some(filename) = file.file_name().and_then(|n| n.to_str()) {
            debug!("  [{}]: {}", i, filename);
        }
    }
    if media_files.len() > 10 {
        debug!("...and last few files:");
        for (i, file) in media_files.iter().enumerate().skip(media_files.len().saturating_sub(3)) {
            if let Some(filename) = file.file_name().and_then(|n| n.to_str()) {
                debug!("  [{}]: {}", i, filename);
            }
        }
    }
    
    // Create keybind profile
    let keybind_profile = KeybindProfile::default();
    let keybind_path = keybind_profile.create_temp_config()?;
    
    // Launch MPV with unique socket for each user
    let socket_path = PathBuf::from(format!("/tmp/syncread_{}.socket", user_id));
    debug!("🔌 User '{}' will use MPV socket: {:?}", user_id, socket_path);
    
    let mpv_controller = MpvController::launch(
        &socket_path,
        Some(&keybind_path),
        media_files.iter().collect(),
    ).await?;
    
    info!("MPV launched successfully!");
    
    // Connect to sync server
    let mut sync_client = SyncClient::new(user_id);
    sync_client.connect_and_sync(server_addr, mpv_controller, media_files).await?;
    
    Ok(())
}

async fn test_mpv_controller(files: Vec<PathBuf>) -> Result<()> {
    info!("Testing MPV controller...");

    // Expand directories and validate files
    let media_files = if files.is_empty() {
        // Default test files
        vec![PathBuf::from("/dev/null")]
    } else {
        expand_media_files(files)?
    };
    
    if media_files.is_empty() {
        anyhow::bail!("No media files found for testing");
    }
    
    info!("Testing with {} files", media_files.len());

    // Create keybind profile
    let keybind_profile = KeybindProfile::default();
    let keybind_path = keybind_profile.create_temp_config()?;

    // Socket path in /tmp
    let socket_path = PathBuf::from("/tmp/syncread_mpv.socket");

    info!("Launching MPV with socket: {:?}", socket_path);
    info!("Keybind config at: {:?}", keybind_path);
    info!("Keybind config exists: {}", keybind_path.exists());

    // Launch MPV
    let mut controller = MpvController::launch(
        &socket_path,
        Some(&keybind_path),
        media_files.iter().collect(),
    ).await?;

    info!("MPV launched successfully!");

    // Test basic commands
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    info!("Testing MPV commands...");

    // Get initial state
    let pos = controller.get_position().await?;
    let playlist_pos = controller.get_playlist_pos().await?;
    let paused = controller.is_paused().await?;

    info!(
        "Initial state - Position: {:.2}s, Playlist: {}, Paused: {}",
        pos, playlist_pos, paused
    );

    // Test play/pause
    if paused {
        info!("Starting playback...");
        controller.play().await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        info!("Pausing playback...");
        controller.pause().await?;
    }

    info!("MPV controller test completed!");
    info!("MPV should be running. Press 'q' in MPV to quit, or Ctrl+C here.");

    // Keep the program running so you can interact with MPV
    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");

    Ok(())
}

/// Expand directories and filter for media files
fn expand_media_files(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    let mut media_files = Vec::new();
    
    for path in paths {
        if path.is_file() {
            media_files.push(path);
        } else if path.is_dir() {
            // Read directory and add media files
            let entries = std::fs::read_dir(&path)
                .with_context(|| format!("Failed to read directory: {:?}", path))?;
                
            let mut dir_files: Vec<PathBuf> = entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|p| p.is_file() && is_media_file(p))
                .collect();
                
            dir_files.sort(); // Sort for consistent ordering
            media_files.extend(dir_files);
        } else {
            anyhow::bail!("Path does not exist: {:?}", path);
        }
    }
    
    Ok(media_files)
}

/// Check if a file appears to be a media file based on extension
fn is_media_file(path: &PathBuf) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let ext = ext.to_lowercase();
        matches!(ext.as_str(), 
            "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "tiff" |
            "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" |
            "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac"
        )
    } else {
        false
    }
}
