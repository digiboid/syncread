# SyncRead Development Commands

# List available commands
default:
    @just --list

# Run the application
run *args:
    cargo run {{args}}

# Run tests
test:
    cargo test

# Check code without building
check:
    cargo check

# Format code
fmt:
    cargo fmt

# Run clippy linter
lint:
    cargo clippy -- -D warnings

# Clean build artifacts
clean:
    cargo clean

# Build release version
build-release:
    cargo build --release

# Start mpv with IPC socket for testing
mpv-test file="/dev/null":
    mpv --input-ipc-server=/tmp/mpvsocket --idle=yes {{file}}

# Test mpv IPC connection
test-mpv-ipc:
    echo '{"command": ["get_property", "playback-time"]}' | socat - /tmp/mpvsocket

# Generate Cargo.lock (useful after adding dependencies)
update-lock:
    cargo update

# Watch for changes and run checks
watch:
    cargo watch -x check -x test
