# programs

standalone `usefossa/programs` repository. on-chain pinocchio program: `src/lib.rs`; account and instruction abi: `src/abi.rs`; active receipt implementation: `src/receipt_prototype.rs`.

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
- operator examples live in `tooling/config.example.toml`; receipt benchmark examples live in `tests/stress/config.example.toml`.
- pass an explicit ignored local configuration through `FOSSA_CONFIG`. examples contain placeholders only; do not point verification at live operator credentials.
- `FOSSA_SBF` selects a program artifact. `LEGACY_TOKEN_SBF` supplies the legacy spl-token artifact for stress checks and legacy vm scenarios; `FOSSA_LEGACY_TOKEN` enables the legacy vm scenario.
- use bun for javascript/typescript dependencies. python uses uv, httpx, and loguru.

## verification and releases

- requested commit/push requires relevant checks and one final diff review. program checks: `cargo test --locked`; stress checks: `cargo test --locked --manifest-path tests/stress/Cargo.toml`, with `FOSSA_SBF` and `LEGACY_TOKEN_SBF` set to existing program and token artifacts.
- local vm harness: `cargo run --locked --manifest-path tests/vm/Cargo.toml`; requires an existing sbf artifact or `FOSSA_SBF`. receipt benchmarks also require an explicit `FOSSA_CONFIG`.
- tooling dependencies: `cd tooling && bun install --frozen-lockfile`; ordinary tooling tests must not load operator configuration or sign/broadcast transactions.
- bump program and tooling semver together for source/tooling changes. commits use `[fix]`, `[feat]`, `[add]`, or `[update]`, with a short lowercase single line and no attribution. tag requested releases `vX.Y.Z`.
