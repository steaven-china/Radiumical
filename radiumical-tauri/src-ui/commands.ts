export interface Command {
  name: string;
  description: string;
  category: string;
  action: "client" | "passthrough";
}

export const commands: Command[] = [
  { name: "/help", description: "Show help", category: "General", action: "client" },
  { name: "/new", description: "Start a new session", category: "Session", action: "client" },
  { name: "/clear", description: "Clear the chat", category: "Session", action: "client" },
  { name: "/sessions", description: "Open session manager", category: "Session", action: "client" },
  { name: "/session save", description: "Save current session", category: "Session", action: "client" },
  { name: "/session list", description: "List saved sessions", category: "Session", action: "client" },
  { name: "/auto", description: "Switch to Auto mode", category: "Mode", action: "client" },
  { name: "/plan", description: "Switch to Plan mode", category: "Mode", action: "client" },
  { name: "/exec", description: "Switch to Exec mode", category: "Mode", action: "client" },
  { name: "/think low", description: "Low thinking effort", category: "Thinking", action: "passthrough" },
  { name: "/think high", description: "High thinking effort", category: "Thinking", action: "passthrough" },
  { name: "/think max", description: "Maximum thinking effort", category: "Thinking", action: "passthrough" },
  { name: "/review", description: "Self-review changes", category: "Agent", action: "passthrough" },
  { name: "/agents", description: "List available agents", category: "Agent", action: "passthrough" },
  { name: "/tools", description: "List available tools", category: "Info", action: "passthrough" },
  { name: "/skills", description: "List available skills", category: "Info", action: "passthrough" },
  { name: "/models", description: "List available models", category: "Info", action: "passthrough" },
  { name: "/model", description: "Set model (e.g. /model gpt-4o)", category: "Config", action: "client" },
  { name: "/settings", description: "Open settings", category: "General", action: "client" },
  { name: "/provider", description: "Open provider picker", category: "General", action: "client" },
  { name: "/perf", description: "Show performance stats", category: "Info", action: "passthrough" },
  { name: "/memory", description: "Show memory", category: "Agent", action: "passthrough" },
  { name: "/mcp", description: "Show MCP server status", category: "Info", action: "passthrough" },
  { name: "/retry", description: "Retry last request", category: "Session", action: "client" },
  { name: "/copy", description: "Copy last response", category: "Utility", action: "passthrough" },
  { name: "/status", description: "Show status info", category: "Info", action: "passthrough" },
  { name: "/outline", description: "Show code diagnostics", category: "Info", action: "passthrough" },
  { name: "/subagents", description: "List sub-agents", category: "Info", action: "passthrough" },
  { name: "/tips", description: "Show tips", category: "General", action: "passthrough" },
];

export function filterCommands(query: string): Command[] {
  const q = query.toLowerCase().replace(/^\//, "");
  if (!q) return commands;
  return commands.filter(
    (c) =>
      c.name.toLowerCase().includes(q) ||
      c.description.toLowerCase().includes(q) ||
      c.category.toLowerCase().includes(q)
  );
}

export function matchExactCommand(text: string): Command | undefined {
  return commands.find((c) => c.name === text.split(" ")[0] || c.name === text);
}
