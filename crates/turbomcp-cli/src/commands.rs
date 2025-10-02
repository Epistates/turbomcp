//! Command implementations that orchestrate transport operations

use crate::cli::{Connection, TransportKind, determine_transport};
use crate::output;
use crate::transports::{http, stdio, ws};

/// Execute tools/list command across any transport
pub async fn tools_list(conn: Connection) -> Result<(), String> {
    let transport = determine_transport(&conn);
    let response = match transport {
        TransportKind::Stdio => stdio::list_tools(&conn).await?,
        TransportKind::Ws => ws::list_tools(&conn).await?,
        TransportKind::Http => http::list_tools(&conn).await?,
    };
    output::display(&conn, &response)
}

/// Execute tools/call command across any transport
pub async fn tools_call(conn: Connection, name: String, arguments: String) -> Result<(), String> {
    let transport = determine_transport(&conn);
    let response = match transport {
        TransportKind::Stdio => stdio::call_tool(&conn, name, arguments).await?,
        TransportKind::Ws => ws::call_tool(&conn, name, arguments).await?,
        TransportKind::Http => http::call_tool(&conn, name, arguments).await?,
    };
    output::display(&conn, &response)
}

/// Execute schema export command across any transport
pub async fn schema_export(conn: Connection, output_path: Option<String>) -> Result<(), String> {
    // Get schema data from appropriate transport
    let transport = determine_transport(&conn);
    let schema_data = match transport {
        TransportKind::Stdio => stdio::get_schemas(&conn).await?,
        TransportKind::Ws => ws::get_schemas(&conn).await?,
        TransportKind::Http => http::get_schemas(&conn).await?,
    };

    // Output to file or stdout
    if let Some(path) = output_path {
        use std::fs;
        let pretty_json = serde_json::to_string_pretty(&schema_data)
            .map_err(|e| format!("Failed to format JSON: {e}"))?;
        fs::write(&path, pretty_json).map_err(|e| format!("Failed to write to {}: {e}", path))?;
        eprintln!("Schemas exported to {}", path);
    } else {
        output::display(&conn, &schema_data)?;
    }

    Ok(())
}
