---
name: aikit-factory-telemetry-collect
description: "METHOD: Collect bounded Factory project signals through Factory's native telemetry owner, then publish its current field to project-scoped Redis with revision checks."
---

# Factory telemetry collection

This Method runs through AIKit's native Routine runner. Its scheduled body asks Factory to collect under the bound project policy, then reads Factory's current field and publishes a bounded Redis projection. Factory owns the durable signals, collections, dispositions and source cursors. Redis is a current reading only; a missing projection is rebuilt from Factory.

The schedule must match the collect schedule in the bound policy. The Routine's grant must cover Factory telemetry collect and field actions. The runner reads the Redis version before reading Factory and uses compare-and-swap to reject a stale publisher. Inspect the native run receipt and Factory source when a run fails; do not treat the Redis projection as source.
