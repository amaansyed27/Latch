import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import * as z from 'zod/v4';

const server = new McpServer({ name: 'latch-scale-fixture', version: '1.0.0' });
let callCount = 0;

for (let index = 0; index < 250; index += 1) {
  const suffix = String(index).padStart(3, '0');
  server.registerTool(
    `benchmark_tool_${suffix}`,
    {
      description: `Synthetic benchmark tool ${suffix} for Latch lazy MCP discovery.`,
      inputSchema: { value: z.string().max(4096).optional() },
    },
    async ({ value }) => {
      callCount += 1;
      return {
        content: [{ type: 'text', text: `scale:${suffix}:${value ?? ''}` }],
        structuredContent: {
          index,
          value: value ?? null,
          pid: process.pid,
          call_count: callCount,
        },
      };
    },
  );
}

await server.connect(new StdioServerTransport());
