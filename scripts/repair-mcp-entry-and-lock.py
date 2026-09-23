from pathlib import Path

path = Path("crates/latch-mcp-client/src/lib.rs")
text = path.read_text()
old = """    let connection_missing = state.connections.get(&server.server_id).is_none();
    if connection_missing {
        let service = connect(server).await?;
        let tools = fetch_catalogue(&service).await?;
        let catalogue_hash = catalogue_hash(&tools);
        state.connections.insert(
            server.server_id,
            Connection {
                fingerprint,
                service,
                tools,
                catalogue_version: 1,
                catalogue_hash,
            },
        );
    }
"""
new = """    if let std::collections::hash_map::Entry::Vacant(entry) =
        state.connections.entry(server.server_id)
    {
        let service = connect(server).await?;
        let tools = fetch_catalogue(&service).await?;
        let catalogue_hash = catalogue_hash(&tools);
        entry.insert(Connection {
            fingerprint,
            service,
            tools,
            catalogue_version: 1,
            catalogue_hash,
        });
    }
"""
if text.count(old) != 1:
    raise SystemExit("expected MCP connection insertion block was not found exactly once")
path.write_text(text.replace(old, new, 1))
