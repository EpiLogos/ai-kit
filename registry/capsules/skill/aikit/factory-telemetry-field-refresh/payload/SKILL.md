---
name: aikit-factory-telemetry-field-refresh
description: "METHOD: Read Factory's current project telemetry field and refresh its project-scoped Redis projection through a native Routine."
---

# Factory telemetry field refresh

This Method runs through AIKit's native Routine runner. It reads the Factory-owned current field for the bound project and publishes that bounded reading to Redis. It does not collect external sources or mutate Factory's durable state.

The schedule must match the field-refresh schedule in the bound policy. The Routine's grant covers only Factory telemetry field. A stale compare-and-swap loses safely; the next tick reads Factory again. A missing Redis projection can always be rebuilt from Factory.
