//! The specimen connector: a complete, deliberately tiny out-of-process
//! connector speaking `aikit.gateway-connector-wire/v1` on stdio.
//!
//! It proves the public out-of-process seam end to end: hello with a declared
//! descriptor (send and typing only — edit and react are deliberately not
//! advertised, so an operation needing them must fail conformance), inbound
//! text events on a schedule, a delivery receipt for every executed operation
//! with a marker proving execution, a health frame on each executed operation,
//! and a clean shutdown.
//!
//! The thin binary wrapper lives with the `aikit` package so its conformance
//! suite can spawn it; the protocol lives here, next to the wire host.

use std::{
    io::{BufRead, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use serde_json::json;

use crate::gateway_connector::{
    ConnectorCapabilities, ConnectorDescriptor, ConnectorHealth, ConnectorHello,
    ConnectorOperation, ConnectorWireFrame, DeliveryReceipt, DeliveryState, InboundEvent,
    InboundEventKind, OutboundOperation, OutboundOperationKind, SenderIdentity, SenderKind,
    GATEWAY_CONNECTOR_SDK_VERSION, GATEWAY_CONNECTOR_WIRE_VERSION,
};

pub const SPECIMEN_CONNECTOR_VERSION: &str = "aikit.gateway-connector-specimen/v1";

/// Everything the specimen needs to introduce itself and what it emits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecimenOptions {
    pub connector_ref: String,
    pub platform: String,
    pub conversation_id: String,
    /// Inbound texts to emit; the same text twice is the same native event,
    /// which is exactly what the duplicate-suppression proof needs.
    pub emit_inbound: Vec<String>,
    /// Delay between consecutive inbound emissions, after the first.
    pub emit_inbound_interval_ms: u64,
    pub health_detail: Option<String>,
}

impl Default for SpecimenOptions {
    fn default() -> Self {
        Self {
            connector_ref: "gateway-connector/specimen/main".into(),
            platform: "specimen".into(),
            conversation_id: "main".into(),
            emit_inbound: Vec::new(),
            emit_inbound_interval_ms: 0,
            health_detail: None,
        }
    }
}

struct SharedWriter<W: Write> {
    inner: Mutex<W>,
}

impl<W: Write> SharedWriter<W> {
    fn write_frame(&self, frame: &ConnectorWireFrame) -> Result<()> {
        let encoded = serde_json::to_string(frame).map_err(|error| {
            AikitError::new("gateway_connector_specimen.encode", error.to_string())
        })?;
        let mut inner = self.inner.lock().expect("specimen writer");
        writeln!(inner, "{encoded}").map_err(|error| {
            AikitError::new(
                "gateway_connector_specimen.write",
                format!("specimen cannot write its frame: {error}"),
            )
        })?;
        inner.flush().map_err(|error| {
            AikitError::new(
                "gateway_connector_specimen.write",
                format!("specimen cannot flush its frame: {error}"),
            )
        })
    }
}

/// Run the specimen protocol until stdin ends or a shutdown frame arrives.
pub fn run_specimen_connector<R: BufRead, W: Write + Send + 'static>(
    input: R,
    output: W,
    options: SpecimenOptions,
) -> Result<()> {
    let connector_ref = ResourceRef::parse(&options.connector_ref)?;
    if options.platform.trim().is_empty() {
        return Err(AikitError::new(
            "gateway_connector_specimen.platform",
            "the specimen connector needs a platform",
        ));
    }
    let descriptor = ConnectorDescriptor {
        version: GATEWAY_CONNECTOR_SDK_VERSION.into(),
        connector_ref: connector_ref.clone(),
        platform: options.platform.clone(),
        implementation: SPECIMEN_CONNECTOR_VERSION.into(),
        // Send and typing only: an operation needing edit/react must fail
        // conformance against this descriptor.
        capabilities: ConnectorCapabilities {
            operations: [ConnectorOperation::Send, ConnectorOperation::Typing].into_iter().collect(),
            max_text_bytes: Some(16_384),
            max_media_bytes: None,
            media_types: Default::default(),
            provenance: vec!["specimen connector".into()],
        },
        configuration_ref: None,
        provenance: vec![SPECIMEN_CONNECTOR_VERSION.into()],
    };
    let writer = Arc::new(SharedWriter { inner: Mutex::new(output) });
    writer.write_frame(&ConnectorWireFrame::Hello {
        hello: ConnectorHello {
            wire_version: GATEWAY_CONNECTOR_WIRE_VERSION.into(),
            descriptor: descriptor.clone(),
        },
    })?;
    writer.write_frame(&ConnectorWireFrame::Health {
        health: ConnectorHealth {
            connector_ref: connector_ref.clone(),
            state: crate::gateway_connector::ConnectorConnectionState::Connected,
            detail: Some(
                options
                    .health_detail
                    .clone()
                    .unwrap_or_else(|| "specimen connector started".into()),
            ),
            provenance: vec![SPECIMEN_CONNECTOR_VERSION.into()],
        },
    })?;

    let stop = Arc::new(AtomicBool::new(false));
    let emitter: Option<JoinHandle<()>> = if options.emit_inbound.is_empty() {
        None
    } else {
        let writer = Arc::clone(&writer);
        let stop = Arc::clone(&stop);
        let connector_ref = connector_ref.clone();
        let platform = options.platform.clone();
        let conversation_id = options.conversation_id.clone();
        let texts = options.emit_inbound.clone();
        let interval = options.emit_inbound_interval_ms;
        Some(thread::spawn(move || {
            for (index, text) in texts.iter().enumerate() {
                if index > 0 && interval > 0 {
                    // Wake often so a shutdown is never a schedule
                    // long-way-round.
                    for _ in 0..(interval / 25).max(1) {
                        if stop.load(Ordering::SeqCst) {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(25));
                    }
                }
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                let sequence = index + 1;
                let event = InboundEvent {
                    event_ref: ResourceRef::parse(format!(
                        "gateway-ingress/specimen/{sequence}"
                    ))
                    .expect("fixed specimen ingress ref"),
                    connector_ref: connector_ref.clone(),
                    address: crate::gateway_connector::ConversationAddress {
                        platform: platform.clone(),
                        scope_id: None,
                        conversation_id: conversation_id.clone(),
                        thread_id: None,
                    },
                    sender: SenderIdentity {
                        native_sender_id: "specimen-human".into(),
                        kind: SenderKind::Human,
                        display_name: Some("Specimen".into()),
                        metadata: Default::default(),
                    },
                    kind: InboundEventKind::Message,
                    custom_kind: None,
                    // Same text, same native event: the specimen's
                    // duplicate for the suppression proof.
                    native_event_id: Some(format!("specimen-{text}")),
                    native_message_id: Some(format!("specimen-message-{sequence}")),
                    reply_to_native_message_id: None,
                    text: Some(text.clone()),
                    media: Vec::new(),
                    observed_at: None,
                    native: Default::default(),
                    provenance: vec![SPECIMEN_CONNECTOR_VERSION.into()],
                };
                if writer
                    .write_frame(&ConnectorWireFrame::Inbound { event })
                    .is_err()
                {
                    return;
                }
            }
        }))
    };

    let mut executed_operations = 0u64;
    let loop_exit: Result<()> = Ok(());
    for line in input.lines() {
        let Ok(line) = line else {
            break;
        };
        if line.trim().is_empty() {
            continue;
        }
        let frame: ConnectorWireFrame = match serde_json::from_str(line.trim()) {
            Ok(frame) => frame,
            Err(_) => continue,
        };
        match frame {
            ConnectorWireFrame::Outbound { operation } => {
                executed_operations += 1;
                let receipt = execute_specimen_operation(&operation, executed_operations);
                let receipt_written =
                    writer.write_frame(&ConnectorWireFrame::DeliveryReceipt { receipt });
                // Health on a signal: every executed operation reports health.
                let health_written = writer.write_frame(&ConnectorWireFrame::Health {
                    health: ConnectorHealth {
                        connector_ref: connector_ref.clone(),
                        state: crate::gateway_connector::ConnectorConnectionState::Connected,
                        detail: Some(format!(
                            "specimen executed {executed_operations} operation(s)"
                        )),
                        provenance: vec![SPECIMEN_CONNECTOR_VERSION.into()],
                    },
                });
                if receipt_written.is_err() || health_written.is_err() {
                    break;
                }
            }
            ConnectorWireFrame::Shutdown { .. } => {
                // An explicit shutdown interrupts any remaining scheduled
                // emissions; a natural stdin EOF lets them finish.
                stop.store(true, Ordering::SeqCst);
                break;
            }
            _ => {}
        }
    }
    if let Some(handle) = emitter {
        let _ = handle.join();
    }
    loop_exit
}

fn execute_specimen_operation(
    operation: &OutboundOperation,
    sequence: u64,
) -> DeliveryReceipt {
    let (state, detail, native_message_id) = match &operation.operation {
        OutboundOperationKind::Send { text, .. } => (
            DeliveryState::Delivered,
            format!(
                "specimen executed send: {}",
                text.as_deref().unwrap_or("")
            ),
            Some(format!("specimen-message-out-{sequence}")),
        ),
        OutboundOperationKind::Typing { active } => (
            DeliveryState::Delivered,
            format!("specimen typing pulse: active={active}"),
            None,
        ),
        other => (
            DeliveryState::Failed,
            format!("specimen does not implement {other:?}"),
            None,
        ),
    };
    let echo = match &operation.operation {
        OutboundOperationKind::Send { text, .. } => json!({ "echo": text }),
        _ => json!({}),
    };
    DeliveryReceipt {
        operation_ref: operation.operation_ref.clone(),
        connector_ref: operation.connector_ref.clone(),
        state,
        native_message_id,
        detail: Some(detail),
        native: [
            ("specimen_marker".to_owned(), json!(format!("executed-{sequence}"))),
            ("specimen".to_owned(), echo),
        ]
        .into_iter()
        .collect(),
        provenance: vec![SPECIMEN_CONNECTOR_VERSION.into()],
    }
}
