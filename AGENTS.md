# programs

standalone `usefossa/programs` repository. on-chain pinocchio program: `src/lib.rs`; account and instruction abi: `src/abi.rs`; active receipt implementation: `src/receipt.rs`. provider onboarding tools and metadata schemas belong in sibling `providers/`; `ops/` here contains only program upgrade tooling.

## workflow

- read relevant implementation before editing. ask when unsure; keep changes concise and latency first.
- no commits, pushes, documentation files, programs, or subagents unless requested. quick edits end without automatic checks.
- keep this repository private unless explicitly asked to change visibility. never publish packages without an explicit request.
- no browser screenshots. prose and comments lowercase; preserve normal casing in identifiers, commands, urls, and user-facing text.
- deployment, upgrades, onboarding, signing, and fund movement require explicit authorization. repository maintenance does not authorize on-chain operations.
- preserve local configuration, encrypted signer records, keys, build artifacts, and test ledgers. never commit credentials, private state, or generated artifacts.

## protocol principles

- keep inference direct between client and provider, with replaceable application infrastructure.
- minimize integration work without inflating compute, cost, or latency; favor reusable setup and concise implementations.
- provider identity and offers come from on-chain state and authorized metadata updates.
- evaluate provider effort, infrastructure burden, trust, spent fees, refundable rent, collateral, compute, latency, and throughput separately.
- account layouts and instruction encodings have external consumers. preserve compatibility and coordinate changes with affected integrations.
- distinguish protocol rules, client behavior, and verified deployments. local measurements do not establish mainnet performance.

## configuration

- on-chain code reads accounts and instruction data, never toml or backend configuration.
- upgrade operator examples live in `ops/config.example.toml`. pass `--config <path>` or `FOSSA_CONFIG`; paths inside config resolve relative to that file. use standard solana json keypairs. commands plan/inspect by default; `--execute` enables writes. provider onboarding and metadata validation use sibling `providers/cli/`.
- examples contain placeholders only; do not point verification at live operator credentials. keep signer files outside git; preserve upgrade buffer keypairs for recovery.
- `FOSSA_SBF` optionally overrides the built program artifact. litesvm bundles both token implementations; `LEGACY_TOKEN_SBF` optionally overrides legacy token code, and `FOSSA_LEGACY_TOKEN` enables the legacy vm smoke scenario.
- receipt benchmarks use built-in `tests/stress/config.example.toml` defaults and local vm rent. `FOSSA_BENCH_CONFIG`, `FOSSA_RECEIPT_BASELINE`, `FOSSA_RECEIPT_PROTOTYPE` and `FOSSA_RECEIPT_OUTPUT` optionally override benchmark inputs/output.
- use bun for javascript/typescript dependencies. python uses uv, httpx, and loguru.

## verification and releases

- requested commit/push requires relevant checks and one final diff review. install ops dependencies with `cd ops && bun install --frozen-lockfile`; root `bun run test` runs offline program/ops unit tests. ops typecheck: `cd ops && bun run check`.
- root `bun run test:vm` builds sbf and runs receipt integration tests plus vm smoke checks. `bun run test:stress` and `bun run bench:receipts` opt into heavier workloads. sbf checks require the solana build tools; no redis, api service, wallet or live rpc is needed. historical binary comparisons remain explicitly ignored without their requested artifact.
- direct rust checks: `cargo test --locked`; after `cargo build-sbf -- --locked`, `cargo test --locked --manifest-path tests/stress/Cargo.toml` and `cargo run --locked --manifest-path tests/vm/Cargo.toml` use generated local fixtures.
- ops tests must not read operator configuration or real signer files, access live rpc, or broadcast transactions. use temporary fixtures and synthetic keys for offline signing checks.
- bump program and ops semver together for source/ops changes. commits use `[fix]`, `[feat]`, `[add]`, or `[update]`, with a short lowercase single line and no attribution. tag requested releases `vX.Y.Z`.
