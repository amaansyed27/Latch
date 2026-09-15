import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import * as z from 'zod/v4';

const server = new McpServer({ name: 'latch-local-fixture', version: '1.0.0' });

server.registerTool(
  'echo',
  {
    description: 'Echo a value through the local MCP fixture.',
    inputSchema: { value: z.string().max(4096) },
  },
  async ({ value }) => ({
    content: [{ type: 'text', text: `local-mcp:${value}` }],
    structuredContent: { echoed: value, source: 'latch-local-fixture' },
  }),
);

await server.connect(new StdioServerTransport());
