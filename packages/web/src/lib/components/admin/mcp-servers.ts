/** How nolune reaches an MCP server, as the form asks it: a command it runs, or an address. */
export type McpKind = 'stdio' | 'remote';

/** What saving, checking or removing an MCP server says, for the row it names (`''`: the add form). */
export interface McpServerResult {
	mcpServer?: string;
	mcpMessage?: string;
	mcpWarning?: string;
	mcpError?: string;
}
