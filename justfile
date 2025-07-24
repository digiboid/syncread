# SyncRead Development Commands

# List available commands
default:
    @just --list

# Run the application
run *args:
    cargo run {{args}}

# Run with sample media files (adjust paths as needed)
run-with-media:
    cargo run

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
    rm -f /tmp/syncread_*.socket
    rm -f /tmp/syncread_keybinds.conf

# Build release version
build-release:
    cargo build --release

# Start mpv with IPC socket for testing (run in separate terminal)
mpv-test file="/dev/null":
    mpv --input-ipc-server=/tmp/mpvsocket --idle=yes --force-window=yes {{file}}

# Test mpv IPC connection (requires mpv-test to be running)
test-mpv-ipc:
    echo '{"command": ["get_property", "playback-time"]}' | socat - /tmp/mpvsocket

# Monitor MPV socket activity
watch-socket:
    watch -n 1 "ls -la /tmp/*socket 2>/dev/null || echo 'No sockets found'"

# Generate Cargo.lock (useful after adding dependencies)
update-lock:
    cargo update

# Watch for changes and run checks
watch:
    cargo watch -x check -x test

# Clean up all temporary files
cleanup:
    rm -f /tmp/syncread_*.socket
    rm -f /tmp/syncread_*.conf
    rm -f /tmp/mpvsocket
    pkill -f "mpv.*syncread" || true
