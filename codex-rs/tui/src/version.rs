/// The current Codex CLI version as embedded at compile time.
pub const CODEX_CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

/// This fork has no compatible automatic upstream update channel.
#[cfg(not(debug_assertions))]
pub const OFFICIAL_UPDATE_CHANNEL_ENABLED: bool = false;
