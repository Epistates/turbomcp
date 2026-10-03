# /// script
# requires-python = ">=3.12"
# dependencies = ["mcp==2.3.0", "uvicorn"]
# ///
"""A Python SDK MCP server with one `add` tool, on Streamable HTTP.

Listens on an ephemeral port and prints `READY <port>` once serving.
"""

import asyncio
import socket

import uvicorn
from mcp.server.mcpserver import MCPServer

server = MCPServer("py-adder")


@server.tool()
def add(a: int, b: int) -> str:
    """Add two integers."""
    return str(a + b)


async def main() -> None:
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.bind(("127.0.0.1", 0))
    # Listening before READY: a client that connects while uvicorn is still
    # starting waits in the backlog instead of being refused.
    sock.listen(128)
    port = sock.getsockname()[1]
    config = uvicorn.Config(server.streamable_http_app(), log_level="warning")
    print(f"READY {port}", flush=True)
    await uvicorn.Server(config).serve(sockets=[sock])


asyncio.run(main())
