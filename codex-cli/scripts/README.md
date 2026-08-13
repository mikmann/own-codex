# npm releases

Use the staging helper in the repo root to generate npm tarballs for a release. For
example, to stage the CLI, responses proxy, and SDK packages for version `0.6.0`:

```bash
./scripts/stage_npm_packages.py \
  --release-version 0.6.0 \
  --package codex \
  --package codex-responses-api-proxy \
  --package codex-sdk
```

This downloads the required native package archive artifacts, hydrates `vendor/` for
each package, and writes tarballs to `dist/npm/`.

In this fork, `--package codex` stages the lightweight
`codex-extended-workflow` meta package plus platform-specific packages of the
same name. The historical `@openai/codex-*` keys are npm dependency aliases
used by the unchanged platform resolver; each alias targets the corresponding
local `codex-extended-workflow` platform version. The primary JS entry point is
`bin/codex-extended-workflow.js`; `bin/codex.js` is retained only as the
disabled-by-default compatibility launcher.

Direct `build_npm_package.py` invocations are still useful for package-specific
debugging, but native packages expect `--vendor-src` to point at a prehydrated
`vendor/` tree. Release packaging should use `scripts/stage_npm_packages.py`.
