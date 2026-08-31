use std::io::{self, BufRead, Write};

use aikit_adapters::{
    execute_gateway_command, AgencyGateway, GatewayRequestEnvelope, GatewayResponseEnvelope,
};
use aikit_core::resource::ResourceRef;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let gateway_ref = std::env::var("AIKIT_GATEWAY_REF")
        .unwrap_or_else(|_| "agency-gateway/local".to_string());
    let gateway_ref = ResourceRef::parse(gateway_ref)?;
    let mut gateway = AgencyGateway::new(gateway_ref);

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