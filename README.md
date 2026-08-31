<p align="center"><strong>codex-extended-workflow</strong> is a compatibility-focused extension of the official OpenAI Codex CLI with deterministic expert, ticket, and review orchestration.
<p align="center">
  <img src="https://github.com/openai/codex/blob/main/.github/codex-cli-splash.png" alt="Codex CLI splash" width="80%" />
</p>
</br>
If you want Codex in your code editor (VS Code, Cursor, Windsurf), <a href="https://developers.openai.com/codex/ide">install in your IDE.</a>
</br>If you want the desktop app experience, run <code>codex-extended-workflow app</code> or visit <a href="https://chatgpt.com/codex?app-landing-page=true">the Codex App page</a>.
</br>If you are looking for the <em>cloud-based agent</em> from OpenAI, <strong>Codex Web</strong>, go to <a href="https://chatgpt.com/codex">chatgpt.com/codex</a>.</p>

---

> [!IMPORTANT]
> This repository is an independently maintained fork, based on the official
> `rust-v0.147.0` tag (commit `be6e8eac029b183056b7e4402879f15d2c85f61b`).
> It is not an official OpenAI release. The requested short tag `v0.147.0` did
> not exist upstream; use of the verified official tag was explicitly approved
> for this fork.

## Extended workflow

The primary executable is `codex-extended-workflow`; the `codex` executable is
retained as a compatibility entry point. The primary launcher enables the
extended workflow only when no CLI, repository, or global setting explicitly
sets `extended_workflow.enabled`; the compatibility launcher preserves the
upstream disabled default. The visible distribution version is
`v0.147.1-alpha-6`. Automatic app-server updates are disabled in the extended
product mode so this fork cannot silently replace itself with an official Codex
package; the compatibility `codex` launcher retains upstream behavior.

Reviewer findings of every severity now require an explicit resolution and one
bounded re-review by the same reviewer. Low and Medium findings may be fixed or
receive a concrete disposition that the reviewer accepts; High and Critical
findings must be fixed. Only a `pass` verdict with an empty finding list closes
the reviewer gate.

By default, the primary launcher keeps local credentials, configuration,
sessions, logs, and SQLite state under `~/.codex-extended-workflow`, separate
from the normal Codex home `~/.codex`. It never imports or falls back to Alpha-2
or normal Codex data automatically. Use `--shared-state` when sharing normal
Codex state and sessions is intentional; credentials and MCP OAuth tokens remain
under `~/.codex-extended-workflow`. Alternatively, set `CODEX_HOME` to an
explicit directory; that explicit home contains both state and credentials.
These two overrides are mutually exclusive.

This separation prevents accidental state reuse; it is not a security boundary
against processes running as the same local user. Alpha-3 does not defend
against symlink, junction, reparse-point, or active path-race manipulation by
such a process.

Prepare a repository safely with a preview first:

```shell
codex-extended-workflow prepare-extend-workflow
codex-extended-workflow prepare-extend-workflow --apply
```

Inside the TUI, use `/prepare-extend-workflow` for the preview and
`/prepare-extend-workflow apply` to apply it. Resume remains compatible:

```shell
codex-extended-workflow resume <session-id>
codex-extended-workflow --shared-state resume <session-id>
```

MultiAgentV2 also exposes `close_agent`: the main agent uses it after a
subagent result has been accepted or terminally rejected to shut that agent's
subtree down, preserve descendant lifecycle state for authorized resume, and
release concurrency slots.

See [Extended workflow architecture and configuration](./EXTENDED_WORKFLOW.md)
for the classifier, expert/reviewer contracts, budget semantics, severity
rules, repository preparation, and implementation references.

## Quickstart

### Installing codex-extended-workflow on Windows

Starting with releases produced by `extended-workflow-release.yml`, download
the installer from the matching GitHub release and run it with the exact tag:

```powershell
$tag = "v0.147.1-alpha-6"
$installer = Join-Path $env:TEMP "install-extended-workflow.ps1"
Invoke-WebRequest "https://github.com/mikmann/own-codex/releases/download/$tag/install-extended-workflow.ps1" -OutFile $installer
& $installer -Release $tag
codex-extended-workflow --version
```

The installer verifies the release checksum, complete package layout, CLI
version, and `codex-code-mode-host` before replacing the managed installation.
Do not use `cargo install` as the end-user installation path: it installs the
CLI crate but not the separate Code Mode host crate.

### Building and running this fork

Build the Rust launcher locally with the repository's normal toolchain:

```shell
cd codex-rs
cargo build --bin codex-extended-workflow
./target/debug/codex-extended-workflow --version
```

The following upstream installation instructions install the official Codex
CLI, not this fork. They are retained as upstream reference material.

### Installing and running the upstream Codex CLI

Run the following on Mac or Linux to install Codex CLI:

```shell
curl -fsSL https://chatgpt.com/codex/install.sh | sh
```

Run the following on Windows to install Codex CLI:

```shell
powershell -ExecutionPolicy ByPass -c "irm https://chatgpt.com/codex/install.ps1 | iex"
```

The standalone installers download from `https://releases.openai.com/codex` by default and fall back to GitHub Releases if a metadata or asset download is unavailable. To force GitHub Releases, set `CODEX_INSTALLER_USE_RELEASES_OPENAI_COM` to `false` (`0` and `no` are also accepted):

```shell
curl -fsSL https://chatgpt.com/codex/install.sh | CODEX_INSTALLER_USE_RELEASES_OPENAI_COM=false sh
```

```powershell
$env:CODEX_INSTALLER_USE_RELEASES_OPENAI_COM='false'; irm https://chatgpt.com/codex/install.ps1 | iex
```

Codex CLI can also be installed via the following package managers:

```shell
# Install using npm
npm install -g @openai/codex
```

```shell
# Install using Homebrew
brew install --cask codex
```

Then simply run `codex` to get started.

<details>
<summary>You can also go to the <a href="https://github.com/openai/codex/releases/latest">latest GitHub Release</a> and download the appropriate binary for your platform.</summary>

Each GitHub Release contains many executables, but in practice, you likely want one of these:

- macOS
  - Apple Silicon/arm64: `codex-aarch64-apple-darwin.tar.gz`
  - x86_64 (older Mac hardware): `codex-x86_64-apple-darwin.tar.gz`
- Linux
  - x86_64: `codex-x86_64-unknown-linux-musl.tar.gz`
  - arm64: `codex-aarch64-unknown-linux-musl.tar.gz`

Each archive contains a single entry with the platform baked into the name (e.g., `codex-x86_64-unknown-linux-musl`), so you likely want to rename it to `codex` after extracting it.

</details>

### Using Codex with your ChatGPT plan

Run `codex` and select **Sign in with ChatGPT**. We recommend signing into your ChatGPT account to use Codex as part of your Plus, Pro, Business, Edu, or Enterprise plan. [Learn more about what's included in your ChatGPT plan](https://help.openai.com/en/articles/11369540-codex-in-chatgpt).

You can also use Codex with an API key, but this requires [additional setup](https://developers.openai.com/codex/auth#sign-in-with-an-api-key).

## Docs

- [**Codex Documentation**](https://developers.openai.com/codex)
- [**Contributing**](./docs/contributing.md)
- [**Installing & building**](./docs/install.md)
- [**Open source fund**](./docs/open-source-fund.md)

This repository is licensed under the [Apache-2.0 License](LICENSE).
