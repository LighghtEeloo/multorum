//! MCP integration tests.
//!
//! Compiled as a single test binary so that shared support modules are
//! built once and all helpers are reachable without dead-code warnings.

mod support;

mod cli_subprocess;
mod concurrent_dispatch;
mod malformed_args;
mod schema_snapshot;
mod transport;
mod wire_roundtrip;
