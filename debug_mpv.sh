#!/usr/bin/env bash
echo "Testing MPV manually with the same arguments..."
echo "This should create a socket at /tmp/syncread_mpv.socket"
echo ""

# Clean up any existing socket
rm -f /tmp/syncread_mpv.socket

# Run MPV with same arguments as the Rust code
mpv --input-ipc-server=/tmp/syncread_mpv.socket \
    --idle=yes \
    --force-window=yes \
    --input-conf=/tmp/syncread_keybinds.conf \
    /dev/null &

MPV_PID=$!
echo "MPV started with PID: $MPV_PID"

# Wait and check for socket
for i in {1..50}; do
    if [ -S /tmp/syncread_mpv.socket ]; then
        echo "✅ Socket created successfully after ${i} attempts!"
        echo "Socket info:"
        ls -la /tmp/syncread_mpv.socket
        
        # Test the socket
        echo ""
        echo "Testing socket communication..."
        echo '{"command": ["get_property", "idle-active"]}' | socat - /tmp/syncread_mpv.socket
        
        # Clean up
        kill $MPV_PID 2>/dev/null
        rm -f /tmp/syncread_mpv.socket
        echo "✅ Manual test successful!"
        exit 0
    fi
    
    if [ $((i % 10)) -eq 0 ]; then
        echo "Still waiting... attempt $i/50"
    fi
    
    sleep 0.1
done

echo "❌ Socket was not created within timeout"
echo "MPV process status:"
if kill -0 $MPV_PID 2>/dev/null; then
    echo "  - MPV process is still running"
    kill $MPV_PID 2>/dev/null
else
    echo "  - MPV process has exited"
fi

rm -f /tmp/syncread_mpv.socket
exit 1
