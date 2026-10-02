// A TypeScript SDK (v2) MCP server with one `add` tool, on Streamable HTTP.
// Listens on an ephemeral port and prints `READY <port>` once serving.
import { createServer } from 'node:http';
import { createMcpHandler, fromJsonSchema, McpServer } from '@modelcontextprotocol/server';
import { toNodeHandler } from '@modelcontextprotocol/node';

const handler = createMcpHandler(() => {
    const server = new McpServer({ name: 'ts-adder', version: '1.0.0' });
    server.registerTool(
        'add',
        {
            description: 'Add two integers',
            inputSchema: fromJsonSchema({
                type: 'object',
                properties: { a: { type: 'integer' }, b: { type: 'integer' } },
                required: ['a', 'b'],
            }),
        },
        async ({ a, b }) => ({ content: [{ type: 'text', text: String(a + b) }] }),
    );
    return server;
});

const node = toNodeHandler(handler);
const http = createServer((req, res) => void node(req, res));
http.listen(0, '127.0.0.1', () => {
    console.log(`READY ${http.address().port}`);
});
