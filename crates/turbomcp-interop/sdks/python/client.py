# /// script
# requires-python = ">=3.12"
# dependencies = ["mcp==2.3.0"]
# ///
"""A Python SDK MCP client: connects to argv[1] over Streamable HTTP in
argv[2]'s era (`legacy` or `modern`), lists the tools, calls `add(2, 3)`,
and prints what it saw as one JSON line."""

import asyncio
import json
import sys

from mcp import Client


async def main() -> None:
    url, era = sys.argv[1], sys.argv[2]
    mode = "2026-07-28" if era == "modern" else "legacy"
    async with Client(url, mode=mode) as client:
        tools = await client.list_tools()
        result = await client.call_tool("add", {"a": 2, "b": 3})
        print(
            json.dumps(
                {
                    "tools": [t.name for t in tools.tools],
                    "text": result.content[0].text,
                    "isError": bool(result.is_error),
                }
            )
        )


asyncio.run(main())
