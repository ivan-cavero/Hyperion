# ADR 0004 — Multi-threaded region ticking

- Status: **Accepted**
- Date: 2026-08-02

## Context

The performance goal requires using all cores. The vanilla model (a single tick
thread) does not scale. Folia (Java) and MCHPRS (Rust) validate the pattern:
partition the world and tick independent regions in parallel.

## Decision

Simulation uses **region ticking**: the world is partitioned into chunk regions;
each region has its own tick loop (20 TPS); the loops run in a thread pool
(rayon / dedicated threads). **There is no shared data between regions**:
cross-region interactions (portals, teleports) are implemented via messages and
state migration.

## Consequences

- Zero global locks on the tick hot path.
- Rust (ownership + Send/Sync) turns concurrency errors into compile
  errors.
- The plugin API (ADR 0002) must expose a "region context" (like
  RegionScheduler in Folia).
- Redstone and entities that cross region boundaries are a delicate design
  case — documented in Phase 3.
