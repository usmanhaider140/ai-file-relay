//! Native messaging host for AI File Relay.
//!
//! Chrome starts this program and talks to it over stdin/stdout.
//! In M0 it answers `hello` and `ping` by itself. From M2 it forwards
//! messages to the running desktop hub over a local socket.

use std::io;
use std::process::ExitCode;

use relay_nmh::{read_message, write_message};
use relay_protocol::{ExtensionToHub, HubToExtension, PROTOCOL_VERSION};

fn main() -> ExitCode {
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();

    loop {
        let msg = match read_message::<ExtensionToHub>(&mut stdin) {
            Ok(Some(msg)) => msg,
            Ok(None) => return ExitCode::SUCCESS,
            Err(e) => {
                // stdout belongs to Chrome; errors go to stderr.
                eprintln!("relay-nmh: {e}");
                return ExitCode::FAILURE;
            }
        };

        let reply = match msg {
            ExtensionToHub::Hello { .. } => HubToExtension::Welcome {
                hub_version: env!("CARGO_PKG_VERSION").to_string(),
                protocol_version: PROTOCOL_VERSION,
            },
            ExtensionToHub::Ping => HubToExtension::Pong,
            other => HubToExtension::Error {
                message: format!("not handled yet (coming in M2): {other:?}"),
            },
        };

        if let Err(e) = write_message(&mut stdout, &reply) {
            eprintln!("relay-nmh: {e}");
            return ExitCode::FAILURE;
        }
    }
}
