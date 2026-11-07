# programs

## scope

- `router/`: inference router. entrypoint: `router/src/lib.rs`; typed state: `router/src/state_types.rs`; receipt implementation: `router/src/receipt.rs`.
- `dex/`: exchange. entrypoint: `dex/src/lib.rs`; typed state: `dex/src/state_types.rs`.
- `ops/`: explicit program inspection and upgrade tooling.
- `tests/vm/`: shared transaction tests. see [protocol documentation](https://fossa.ag/docs/technical).

## workflow

- dependency updates and security fixes are manual; retain vulnerability alerts and dependency inventories.
- read the relevant implementation before editing. clarify uncertain requirements and keep changes focused.
- deployment, upgrades, onboarding, signing, fund movement, and package publication require explicit authorization. repository maintenance alone grants none.
- preserve local configuration, encrypted signer records, keys, build artifacts, and test ledgers.

## implementation

### protocol principles

- keep inference direct between client and provider, with replaceable application infrastructure.
- minimize integration work without inflating compute units, cost, or latency. favor reusable setup and concise implementations.
- derive provider identities and offers from on-chain state and authorized metadata updates.
- evaluate provider effort, infrastructure burden, trust, spent fees, refundable rent, compute, latency, and throughput separately.
- account layouts and instruction encodings have external consumers. coordinate changes with affected integrations.
- distinguish protocol rules, client behavior, and verified deployments. local measurements do not establish mainnet performance.
- keep readmes to documentation links: one properly capitalized link per component, each on its own line. public documentation explains contract rules and trust boundaries without private operational details.

### program structure

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

## configuration and security

- on-chain code reads accounts and instruction data, never off-chain configuration.
- never log or commit credentials, signer bytes, private state, or generated artifacts. examples contain placeholders only; never point ordinary verification at live operator credentials.
- operator examples live in `ops/config.example.toml`. pass `--config <path>` or `FOSSA_CONFIG`; configured paths resolve relative to that file. use standard Solana JSON keypairs and keep them outside git.
- operator commands plan or inspect by default; only explicit `--execute` enables writes. preserve upgrade buffer keypairs for recovery.
- `FOSSA_SBF` optionally overrides the built program artifact.

## verification

- before a requested commit or push, run relevant checks and review the final diff. root `bun run test` runs offline router, DEX, and ops unit tests. ops setup: `cd ops && bun install --frozen-lockfile`; typecheck: `cd ops && bun run check`.
- direct Rust checks: `cargo test --manifest-path router/Cargo.toml --locked` and `cargo test --manifest-path dex/Cargo.toml --locked`.
- build on-chain artifacts with `cargo build-sbf --manifest-path router/Cargo.toml -- --locked` and `cargo build-sbf --manifest-path dex/Cargo.toml -- --locked`.
- on-chain behavior changes require `bun run test:vm`, which builds both SBF artifacts and runs `tests/vm/`. use generated Anchor clients, synthetic keys, and SPL fixture helpers from `tests/vm/src/tests.rs`.
- the VM harness requires host Rust 1.97.1 or newer. select a compatible host toolchain without changing SBF platform tools.
- keep pure arithmetic and matching tests beside their implementation; keep transaction tests in the shared VM suite. extend existing fixtures and cover changed success/rejection behavior with relevant balance and state assertions. preserve regression coverage when restructuring.
- identify instruction, account-layout, and PDA changes separately from business-rule changes. compilation or a small VM suite alone does not establish migration compatibility, full behavioral parity, or unchanged compute-unit usage.
- ops tests must not read operator configuration or real signer files, access live RPC, or broadcast transactions. use temporary fixtures and synthetic keys for offline signing checks.

## commits and releases

- create commits and push only when explicitly requested.
- use `[type] short description`; allowed types are `[fix]`, `[feat]`, `[add]`, and `[update]`.
- messages must be one lowercase line. combine multiple changes with `+`.
- never add attribution or `co-authored-by` trailers.
- bump affected versions with semver for behavior or release changes. tag requested releases `vX.Y.Z`.
- bump affected program and ops versions together for source or ops changes.
