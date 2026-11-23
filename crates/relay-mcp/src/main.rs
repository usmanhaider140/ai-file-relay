//! MCP server for AI File Relay.
//!
//! M0 placeholder. In M1 this becomes a real MCP server built with `rmcp`,
//! serving the tools below over stdio (and over HTTP inside the hub).

/// Tools planned for v0.1.
const PLANNED_TOOLS: &[(&str, &str)] = &[
    ("list_tabs", "List open browser tabs"),
    ("upload_to_page", "Put a file into a page's upload box"),
    (
        "wait_for_download",
        "Wait for a download and return its path",
    ),
    ("list_files", "List files in an allowed folder"),
    (
        "relay_status",
        "Show whether the hub and extension are connected",
    ),
];

fn main() {
    // Make sure the core crate links; real use comes in M1.
    let folders = relay_core::AllowedFolders::new();
    eprintln!(
        "relay-mcp {}: MCP tools arrive in M1 ({} allowed folders configured).",
        env!("CARGO_PKG_VERSION"),
        folders.roots().len()
    );
    for (name, what) in PLANNED_TOOLS {
        eprintln!("  {name:<18} {what}");
    }
}
