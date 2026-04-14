/// MCP stdio server adapter
/// 
/// Reads JSON-RPC 2.0 requests from stdin, processes through voltnuerongrid-mcp core,
/// and writes responses to stdout. Suitable for MCP client integration (VSCode, Cursor, Claude).
/// 
/// Usage:
/// cargo run -p voltnuerongrid-mcp --bin mcp-stdio-server --release
///
/// Or pre-built:
/// ./target/release/mcp-stdio-server

use std::io::{self, BufRead, Write};
use serde_json::{json, Value};
use voltnuerongrid_mcp::McpRequest;

fn main() {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut stdout = io::stdout();
    let mut line = String::new();

    // Process incoming JSON-RPC 2.0 requests line-by-line
    while let Ok(n) = reader.read_line(&mut line) {
        if n == 0 {
            // EOF reached
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            line.clear();
            continue;
        }

        match serde_json::from_str::<McpRequest>(trimmed) {
            Ok(request) => {
                // Route through mcp core blocking wrapper
                let response = voltnuerongrid_mcp::process_mcp_request_blocking(request);
                
                // Write response to stdout
                if let Ok(json_str) = serde_json::to_string(&response) {
                    let _ = writeln!(stdout, "{}", json_str);
                    let _ = stdout.flush();
                }
            }
            Err(e) => {
                // JSON parse error
                let error_response = json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "code": -32700,
                        "message": "Parse error",
                        "data": e.to_string()
                    },
                    "id": Value::Null
                });
                let _ = writeln!(stdout, "{}", error_response);
                let _ = stdout.flush();
            }
        }

        line.clear();
    }
}
