// A Go SDK MCP server with one `add` tool, on Streamable HTTP. Listens on an
// ephemeral port and prints `READY <port>` once serving.
package main

import (
	"context"
	"fmt"
	"log"
	"net"
	"net/http"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

type addArgs struct {
	A int `json:"a"`
	B int `json:"b"`
}

func add(_ context.Context, _ *mcp.CallToolRequest, args addArgs) (*mcp.CallToolResult, any, error) {
	return &mcp.CallToolResult{
		Content: []mcp.Content{&mcp.TextContent{Text: fmt.Sprint(args.A + args.B)}},
	}, nil, nil
}

func main() {
	server := mcp.NewServer(&mcp.Implementation{Name: "go-adder", Version: "1.0.0"}, nil)
	mcp.AddTool(server, &mcp.Tool{Name: "add", Description: "Add two integers"}, add)
	// Stateless: the Go SDK serves 2026-07-28 only without sessions (its
	// stateful handler lists the 2025 revisions alone), and serves the 2025
	// revisions statelessly beside it.
	handler := mcp.NewStreamableHTTPHandler(
		func(*http.Request) *mcp.Server { return server },
		&mcp.StreamableHTTPOptions{Stateless: true},
	)

	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		log.Fatal(err)
	}
	fmt.Printf("READY %d\n", listener.Addr().(*net.TCPAddr).Port)
	log.Fatal(http.Serve(listener, handler))
}
