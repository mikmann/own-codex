#!/usr/bin/env node
// Primary npm entry point. Platform packages intentionally keep the upstream native filename
// `codex`; this explicit marker selects the extended product defaults without conflating the
// compatibility `codex` shim.

process.env.CODEX_EXTENDED_WORKFLOW_LAUNCHER = "1";
await import("./codex.js");
