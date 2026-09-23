from pathlib import Path

path = Path("crates/latch-mcp-client/src/lib.rs")
text = path.read_text()
old_ok = """            Ok(connection) => result.push(McpProviderInfo {
                server_id: server.server_id,
                connected: !connection.service.peer().is_transport_closed(),
"""
new_ok = """            Ok(connection) => result.push(McpProviderInfo {
                server_id: server.server_id,
                display_name: server.display_name.clone(),
                connected: !connection.service.peer().is_transport_closed(),
"""
old_err = """            Err(_) => result.push(McpProviderInfo {
                server_id: server.server_id,
                connected: false,
"""
new_err = """            Err(_) => result.push(McpProviderInfo {
                server_id: server.server_id,
                display_name: server.display_name.clone(),
                connected: false,
"""
if text.count(old_ok) != 1 or text.count(old_err) != 1:
    raise SystemExit("expected McpProviderInfo initializer fragments were not found exactly once")
text = text.replace(old_ok, new_ok, 1).replace(old_err, new_err, 1)
path.write_text(text)
