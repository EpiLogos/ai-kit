use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
};

use aikit_adapters::{
    execute_gateway_command, run_gateway_service, AgencyGateway, GatewayRequestEnvelope,
    GatewayResponseEnvelope, GatewayServiceConfig, DEFAULT_GATEWAY_MAX_FRAME_BYTES,
};
use aikit_core::resource::ResourceRef;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let gateway_ref =
        std::env::var("AIKIT_GATEWAY_REF").unwrap_or_else(|_| "agency-gateway/local".to_string());
    let gateway = AgencyGateway::new(ResourceRef::parse(gateway_ref)?);
    let args = std::env::args().skip(1).collect::<Vec<_>>();

    if args.first().is_some_and(|arg| arg == "serve") {
        return run_service(gateway, &args[1..]);
    }
    if !args.is_empty() {
        return Err(invalid_input(format!(
            "unknown aikit-gateway arguments: {}; use `aikit-gateway` for stdio or `aikit-gateway serve --ws HOST:PORT [--unix PATH] [--state-file PATH]` for persistent service mode",
            args.join(" ")
        ))
        .into());
    }
    run_stdio(gateway)
}

fn run_service(gateway: AgencyGateway, args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut websocket_bind = None;
    let mut unix_socket = None;
    let mut state_file = None;
    let mut token_env = "AIKIT_GATEWAY_TOKEN".to_string();
    let mut max_frame_bytes = DEFAULT_GATEWAY_MAX_FRAME_BYTES;
    let mut index = 0;
    while index < args.len() {
        let flag = &args[index];
        let value = |index: &mut usize| -> Result<String, io::Error> {
            *index += 1;
            args.get(*index)
                .cloned()
                .ok_or_else(|| invalid_input(format!("{flag} requires a value")))
        };
        match flag.as_str() {
            "--ws" => websocket_bind = Some(value(&mut index)?),
            "--unix" => unix_socket = Some(PathBuf::from(value(&mut index)?)),
            "--state-file" => state_file = Some(PathBuf::from(value(&mut index)?)),
            "--token-env" => token_env = value(&mut index)?,
            "--max-frame-bytes" => {
                let raw = value(&mut index)?;
                max_frame_bytes = raw.parse::<usize>().map_err(|error| {
                    invalid_input(format!("invalid --max-frame-bytes `{raw}`: {error}"))
                })?;
            }
            "--help" | "-h" => {
                println!(
                    "aikit-gateway serve [--ws HOST:PORT] [--unix PATH] [--token-env ENV] [--state-file PATH] [--max-frame-bytes BYTES]\n\nWebSocket mode requires a bearer token in AIKIT_GATEWAY_TOKEN by default. Unix sockets are owner-only (0600). The same gateway command protocol is available on every carrier."
                );
                return Ok(());
            }
            other => return Err(invalid_input(format!("unknown serve option `{other}`")).into()),
        }
        index += 1;
    }

    let websocket_bearer_token = if websocket_bind.is_some() {
        Some(std::env::var(&token_env).map_err(|_| {
            invalid_input(format!(
                "WebSocket service requires bearer token environment variable `{token_env}`"
            ))
        })?)
    } else {
        None
    };
    run_gateway_service(
        gateway,
        GatewayServiceConfig {
            websocket_bind,
            websocket_bearer_token,
            unix_socket,
            state_file,
            max_frame_bytes,
        },
    )?;
    Ok(())
}

fn run_stdio(mut gateway: AgencyGateway) -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout().lock());

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request = match serde_json::from_str::<GatewayRequestEnvelope>(&line) {
            Ok(request) => request,
            Err(error) => {
                let response = serde_json::json!({
                    "request_id": null,
                    "ok": false,
                    "error": {
                        "code": "agency_gateway.invalid_request_json",
                        "message": error.to_string()
                    }
                });
                serde_json::to_writer(&mut stdout, &response)?;
                writeln!(&mut stdout)?;
                stdout.flush()?;
                continue;
            }
        };
        let shutdown = request.command.is_shutdown();
        let response = GatewayResponseEnvelope::from_result(
            request.request_id,
            execute_gateway_command(&mut gateway, request.command),
        );
        serde_json::to_writer(&mut stdout, &response)?;
        writeln!(&mut stdout)?;
        stdout.flush()?;
        if shutdown {
            break;
        }
    }
    Ok(())
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
