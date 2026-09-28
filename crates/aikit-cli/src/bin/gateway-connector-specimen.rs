//! The specimen out-of-process connector: a complete, deliberately tiny
//! `aikit.gateway-connector-wire/v1` speaker on stdio, used to prove the
//! public out-of-process connector seam end to end. The protocol lives in
//! `aikit_adapters::run_specimen_connector`; this wrapper only parses args.
//!
//! It declares send and typing only (deliberately not edit or react), echoes
//! executed sends in its receipts with a marker, reports health on each
//! executed operation, and exits on shutdown.

use std::io::{self, BufReader};

use aikit_adapters::{run_specimen_connector, SpecimenOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = SpecimenOptions::default();
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    while let Some(flag) = args.first().cloned() {
        args.remove(0);
        let value = |args: &mut Vec<String>| -> Result<String, String> {
            if args.is_empty() {
                return Err(format!("{flag} requires a value"));
            }
            Ok(args.remove(0))
        };
        match flag.as_str() {
            "--connector-ref" => options.connector_ref = value(&mut args)?,
            "--platform" => options.platform = value(&mut args)?,
            "--conversation" => options.conversation_id = value(&mut args)?,
            "--emit-inbound" => options.emit_inbound.push(value(&mut args)?),
            "--emit-inbound-delay-ms" => {
                options.emit_inbound_delay_ms = value(&mut args)?.parse().map_err(|_| {
                    String::from("--emit-inbound-delay-ms needs a number of milliseconds")
                })?
            }
            "--emit-inbound-interval-ms" => {
                options.emit_inbound_interval_ms = value(&mut args)?.parse().map_err(|_| {
                    String::from("--emit-inbound-interval-ms needs a number of milliseconds")
                })?
            }
            "--health-detail" => options.health_detail = Some(value(&mut args)?),
            "--help" | "-h" => {
                println!(
                    "gateway-connector-specimen [--connector-ref REF] [--platform NAME] \
                     [--conversation ID] [--emit-inbound TEXT]... [--emit-inbound-delay-ms MS] \
                     [--emit-inbound-interval-ms MS] [--health-detail TEXT]\n\nSpeaks \
                     aikit.gateway-connector-wire/v1 on stdio: hello, inbound events, delivery \
                     receipts with an execution marker, health, shutdown."
                );
                return Ok(());
            }
            other => {
                return Err(format!("unknown specimen option `{other}`; try --help").into());
            }
        }
    }
    let stdin = io::stdin();
    run_specimen_connector(BufReader::new(stdin.lock()), io::stdout(), options)?;
    Ok(())
}
