// A Go SDK MCP client: connects to os.Args[1] over Streamable HTTP in
// os.Args[2]'s era (`legacy` or `modern`), lists the tools, calls
// `add(2, 3)`, and prints what it saw as one JSON line.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"os"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

func main() {
	url, era := os.Args[1], os.Args[2]
	version := "2025-11-25"
	if era == "modern" {
		version = "2026-07-28"
	}
	ctx := context.Background()
	client := mcp.NewClient(&mcp.Implementation{Name: "go-client", Version: "1.0.0"}, nil)
	session, err := client.Connect(ctx, &mcp.StreamableClientTransport{Endpoint: url},
		&mcp.ClientSessionOptions{ProtocolVersion: version})
	if err != nil {
		log.Fatal(err)
	}
	defer session.Close()

	tools, err := session.ListTools(ctx, nil)
	if err != nil {
		log.Fatal(err)
	}
	result, err := session.CallTool(ctx, &mcp.CallToolParams{
		Name:      "add",
		Arguments: map[string]any{"a": 2, "b": 3},
	})
	if err != nil {
		log.Fatal(err)
	}
	names := []string{}
	for _, t := range tools.Tools {
		names = append(names, t.Name)
	}
	text := ""
	if len(result.Content) > 0 {
		if t, ok := result.Content[0].(*mcp.TextContent); ok {
			text = t.Text
		}
	}
	out, _ := json.Marshal(map[string]any{"tools": names, "text": text, "isError": result.IsError})
	fmt.Println(string(out))
}
