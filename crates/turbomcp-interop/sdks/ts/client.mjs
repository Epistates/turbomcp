// A TypeScript SDK (v2) MCP client: connects to argv[2] over Streamable HTTP
// in argv[3]'s era (`legacy` or `modern`), lists the tools, calls
// `add(2, 3)`, and prints what it saw as one JSON line.
import { Client, StreamableHTTPClientTransport } from '@modelcontextprotocol/client';

const [url, era] = process.argv.slice(2);
const client = new Client(
    { name: 'ts-client', version: '1.0.0' },
    { versionNegotiation: { mode: era === 'modern' ? { pin: '2026-07-28' } : 'legacy' } },
);
const transport = new StreamableHTTPClientTransport(new URL(url));
await client.connect(transport);
const tools = await client.listTools();
const result = await client.callTool({ name: 'add', arguments: { a: 2, b: 3 } });
console.log(
    JSON.stringify({
        tools: tools.tools.map((t) => t.name),
        text: result.content?.[0]?.text,
        isError: result.isError ?? false,
    }),
);
await client.close();
