use anyhow::bail;
use std::ffi::OsStr;
use std::ffi::OsString;

const CODEX_HOME_ENV: &str = "CODEX_HOME";
const STATE_SCOPE_ENV: &str = "CODEX_EXTENDED_WORKFLOW_STATE_SCOPE";
const SHARED_STATE_FLAG: &str = "--shared-state";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProductFlavor {
    Extended,
    Compatibility,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SharedStateRequest {
    Default,
    Shared,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StateRootMode {
    Isolated,
    Shared,
    Compatibility,
    Explicit,
}

impl StateRootMode {
    fn env_value(self) -> &'static str {
        match self {
            Self::Isolated => "isolated",
            Self::Shared => "shared",
            Self::Compatibility => "compatibility",
            Self::Explicit => "explicit",
        }
    }
}

pub(crate) fn configure_product_state(flavor: ProductFlavor) -> anyhow::Result<StateRootMode> {
    let request = shared_state_request(std::env::args_os());
    let codex_home = std::env::var_os(CODEX_HOME_ENV).filter(|value| !value.is_empty());
    let mode = resolve_state_root_mode(flavor, request, codex_home.as_deref())?;

    // SAFETY: `main` calls this before `arg0_dispatch_or_else` starts async work or child
    // processes. Child CLIs re-evaluate their own product identity and flags instead of treating
    // this inherited process-local value as a user request.
    unsafe { std::env::set_var(STATE_SCOPE_ENV, mode.env_value()) };
    Ok(mode)
}

fn resolve_state_root_mode(
    flavor: ProductFlavor,
    request: SharedStateRequest,
    codex_home: Option<&OsStr>,
) -> anyhow::Result<StateRootMode> {
    if flavor == ProductFlavor::Compatibility {
        if request == SharedStateRequest::Shared {
            bail!("--shared-state is only supported by codex-extended-workflow");
        }
        return Ok(StateRootMode::Compatibility);
    }

    if request == SharedStateRequest::Shared {
        if codex_home.is_some() {
            bail!("--shared-state cannot be combined with CODEX_HOME");
        }
        return Ok(StateRootMode::Shared);
    }
    if codex_home.is_some() {
        return Ok(StateRootMode::Explicit);
    }

    Ok(StateRootMode::Isolated)
}

fn shared_state_request(args: impl IntoIterator<Item = OsString>) -> SharedStateRequest {
    args.into_iter()
        .skip(1)
        .take_while(|arg| arg != OsStr::new("--"))
        .find(|arg| arg == OsStr::new(SHARED_STATE_FLAG))
        .map_or(SharedStateRequest::Default, |_| SharedStateRequest::Shared)
}

#[cfg(test)]
#[path = "product_state_tests.rs"]
mod tests;
