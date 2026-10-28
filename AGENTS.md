# programs

on-chain anchor v2 programs: `src/lib.rs` and `dex/src/lib.rs`; typed account state: `src/state_types.rs` and `dex/src/state_types.rs`; active receipt implementation: `src/receipt.rs`. `ops/` contains program upgrade tooling.

## workflow

- read relevant implementation before editing. ask when unsure; keep changes concise and latency first.
- no commits, pushes, documentation files, programs, or subagents unless requested. quick edits end without automatic checks.
- never publish packages without an explicit request.
- no browser screenshots. prose and comments lowercase; preserve normal casing in identifiers, commands, urls, and user-facing text.
- deployment, upgrades, onboarding, signing, and fund movement require explicit authorization. repository maintenance does not authorize on-chain operations.
- preserve local configuration, encrypted signer records, keys, build artifacts, and test ledgers. never commit credentials, private state, or generated artifacts.

## protocol principles

- keep inference direct between client and provider, with replaceable application infrastructure.
- minimize integration work without inflating compute, cost, or latency; favor reusable setup and concise implementations.
- provider identity and offers come from on-chain state and authorized metadata updates.
- evaluate provider effort, infrastructure burden, trust, spent fees, refundable rent, compute, latency, and throughput separately.
- account layouts and instruction encodings have external consumers. coordinate changes with affected integrations.
- distinguish protocol rules, client behavior, and verified deployments. local measurements do not establish mainnet performance.

## program structure

- follow the existing anchor v2 architecture in both programs. keep `#[program]` entrypoints in `lib.rs` as thin, typed calls into instruction handlers; keep related business logic together in the existing instruction modules.
- define instruction arguments in `args.rs` with `AnchorSerialize`, `AnchorDeserialize` and `IdlType`. define named account fields in `state_types.rs` with `#[account]` and the existing `PodU*`/`PodI*` types. use normal generated discriminators and serialization rather than custom opcode tables, byte offsets or wire codecs.
- declare account roles in `contexts.rs` with `#[derive(Accounts)]`. express ownership, addresses, signer requirements, seeds, bumps, initialization, payer, space and token relationships through anchor constraints where supported. handlers validate amounts, lifecycle, accounting and other business invariants.
- prefer `Account<T>`, `Signer`, `Program<T>`, `InterfaceAccount<T>` and token interfaces. use `UncheckedAccount` and remaining accounts only for roles that cannot be typed, with explicit validation at the boundary. retain checked cpi handles and borrow guards.
- use `Slab<Header, Entry>` for bounded account collections and `Nested<Context>` for shared account groups, following the current offer books, lists and dex contexts. use framework initialization, sizing and resizing instead of maintaining parallel raw headers or collection formats.
- use `anchor_spl` operations and generated program cpi account/argument types, including the dex crate's `cpi` interface. use generated `InstructionData` and `ToAccountMetas` in clients and tests instead of manually packing instructions or duplicating account ordering.
- use solana/spl ids, state types, extension accessors and codecs for external formats. keep unavoidable format definitions isolated, as in `ops/formats.ts`; do not scatter magic offsets, lengths or raw token/loader decoding through handlers.
- keep raw account access and `unsafe` narrowly scoped to framework or sdk boundaries. explain the ownership, borrowing, initialization or alignment invariant that makes a new unsafe operation valid; prefer an existing typed accessor when available.
- use named `RouterError`/`DexError` variants and small domain helpers. keep integer accounting, rounding, slippage, authorization and replay checks explicit. add abstractions for repeated domain operations rather than building another account, serialization or dispatch framework.
- optimize measured hot paths within this structure. retain readable typed code and validation; justify lower-level replacements with representative sbf measurements. use git history for retired implementations rather than restoring duplicate source trees or compatibility layers without an integration requirement.

## configuration

- on-chain code reads accounts and instruction data, never off-chain configuration.
- upgrade operator examples live in `ops/config.example.toml`. pass `--config <path>` or `FOSSA_CONFIG`; paths inside config resolve relative to that file. use standard solana json keypairs. commands plan/inspect by default; `--execute` enables writes.
- examples contain placeholders only; do not point verification at live operator credentials. keep signer files outside git; preserve upgrade buffer keypairs for recovery.
- `FOSSA_SBF` optionally overrides the built program artifact.
- use bun for javascript/typescript dependencies. python uses uv, httpx, and loguru.

## verification and releases

- requested commit/push requires relevant checks and one final diff review. install ops dependencies with `cd ops && bun install --frozen-lockfile`; root `bun run test` runs offline program/ops unit tests. ops typecheck: `cd ops && bun run check`.
- direct rust checks: `cargo test --locked`; build the on-chain artifacts with `cargo build-sbf -- --locked` and `cargo build-sbf --manifest-path dex/Cargo.toml -- --locked`.
- on-chain behavior changes use `bun run test:vm`, which builds both sbf artifacts and runs the shared litesvm suite in `tests/vm/`. use generated anchor clients, synthetic keys and spl fixture helpers, following `tests/vm/src/tests.rs`.
- the vm harness requires host rust 1.97.1 or newer; select a compatible installed toolchain without changing the sbf platform tools.
- keep pure arithmetic/matching tests beside their implementation and transaction tests in the shared vm suite. add focused success and rejection cases for changed behavior, asserting relevant balances and state. extend existing fixtures; avoid duplicate harnesses and unrelated test matrices. keep regression coverage when restructuring code.
- identify instruction, account layout and pda changes separately from business-rule changes. coordinate affected consumers and migration needs; compilation or a small vm suite alone does not establish migration compatibility, full behavioral parity or unchanged cu usage.
- ops tests must not read operator configuration or real signer files, access live rpc, or broadcast transactions. use temporary fixtures and synthetic keys for offline signing checks.
- bump program and ops semver together for source/ops changes. commits use `[fix]`, `[feat]`, `[add]`, or `[update]`, with a short lowercase single line and no attribution. tag requested releases `vX.Y.Z`.
