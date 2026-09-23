from pathlib import Path

path = Path("crates/latch-mcp-client/src/lib.rs")
text = path.read_text()
text = text.replace("    display_name: String,\n", "")
text = text.replace("                display_name: server.display_name.clone(),\n", "")
text = text.replace(
    "    if !state.connections.contains_key(&server.server_id) {\n",
    "    let connection_missing = state.connections.get(&server.server_id).is_none();\n    if connection_missing {\n",
)
old = """            (!allowed
                .get(server_id)
                .is_some_and(|fingerprint| *fingerprint == connection.fingerprint))
            .then_some(*server_id)
"""
new = """            allowed
                .get(server_id)
                .is_none_or(|fingerprint| *fingerprint != connection.fingerprint)
                .then_some(*server_id)
"""
text = text.replace(old, new)
path.write_text(text)
