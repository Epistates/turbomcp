#!/bin/bash
# Run the sampling demo with server and client connected via pipes

echo "🚀 TurboMCP Sampling Demo"
echo "========================="
echo ""
echo "This demo shows server→client sampling requests in action."
echo "The server provides tools that ask questions to the client's LLM."
echo ""

# Create named pipes for bidirectional communication
PIPE_DIR="/tmp/turbomcp_demo_$$"
mkdir -p "$PIPE_DIR"
SERVER_TO_CLIENT="$PIPE_DIR/server_to_client"
CLIENT_TO_SERVER="$PIPE_DIR/client_to_server"

mkfifo "$SERVER_TO_CLIENT" "$CLIENT_TO_SERVER"

# Cleanup function
cleanup() {
    echo ""
    echo "Cleaning up..."
    rm -rf "$PIPE_DIR"
    kill $SERVER_PID $CLIENT_PID 2>/dev/null
    exit
}

trap cleanup EXIT INT TERM

echo "Starting server and client..."
echo "------------------------------"
echo ""

# Start the server in background
cargo run --example sampling_demo_server < "$CLIENT_TO_SERVER" > "$SERVER_TO_CLIENT" 2>&1 &
SERVER_PID=$!

# Give server a moment to start
sleep 2

# Start the client connected to server
cargo run --example sampling_demo_client < "$SERVER_TO_CLIENT" > "$CLIENT_TO_SERVER" 2>&1 &
CLIENT_PID=$!

echo "Server PID: $SERVER_PID"
echo "Client PID: $CLIENT_PID"
echo ""
echo "Demo is running! The client and server are communicating."
echo "Press Ctrl+C to stop."
echo ""

# Wait for processes
wait $SERVER_PID $CLIENT_PID