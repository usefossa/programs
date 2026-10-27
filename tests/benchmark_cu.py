import argparse
import csv
import hashlib
import json
import platform
import re
import statistics
import subprocess
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path


def fingerprint(path):
    data = path.read_bytes()
    return {"path": str(path), "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def command(*args):
    return subprocess.check_output(args, text=True, stderr=subprocess.STDOUT).strip()


def write_csv(path, rows):
    with path.open("w", newline="") as output:
        writer = csv.DictWriter(output, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--original", type=Path, required=True)
    parser.add_argument("--runs", type=int, default=5)
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    output = args.output.resolve()
    rows, groups = [], defaultdict(list)
    expected = {}
    for suite in ("router", "dex"):
        for run in range(1, args.runs + 1):
            log = (output / f"{suite}-{run}.log").read_text()
            assert ("8 passed; 0 failed" if suite == "router" else "all dex SBF scenarios passed") in log
            with (output / f"{suite}-{run}.csv").open() as source:
                current = list(csv.DictReader(source))
            keys = [(r["fixture"], r["ordinal"], r["label"], r["source"], r["success"]) for r in current]
            if run == 1:
                expected[suite] = keys
            assert keys == expected[suite], f"scenario order/shape changed: {suite} run {run}"
            for row in current:
                assert row["run"] == str(run)
                row["suite"] = suite
                row["token_runtime"] = "spl-token-3.5.0" if "legacy=true" in row["fixture"] else "p-token"
                row["token_2022_runtime"] = "spl-token-2022-11.0.0"
                for field in ("ordinal", "limit_cu", "original_cu", "anchor_cu"):
                    row[field] = int(row[field])
                row["delta_cu"] = row["anchor_cu"] - row["original_cu"]
                row["delta_pct"] = 100 * row["delta_cu"] / row["original_cu"] if row["original_cu"] else None
                row["anchor_headroom_cu"] = row["limit_cu"] - row["anchor_cu"]
                rows.append(row)
                groups[(suite, row["fixture"], row["ordinal"])].append(row)
    summaries = []
    for group in groups.values():
        assert len(group) == args.runs
        first = group[0]
        result = {k: first[k] for k in ("suite", "fixture", "ordinal", "label", "source", "instructions", "migration", "success", "limit_cu", "token_runtime", "token_2022_runtime")}
        result["pairs"] = len(group)
        for field in ("original_cu", "anchor_cu", "delta_cu", "delta_pct", "anchor_headroom_cu"):
            values = [r[field] for r in group if r[field] is not None]
            for name, function in (("median", statistics.median), ("min", min), ("max", max)):
                result[f"{field}_{name}"] = function(values) if values else None
        summaries.append(result)
    write_csv(output / "paired.csv", rows)
    write_csv(output / "scenarios.csv", summaries)

    binaries = ("target/deploy/inference_router_program.so", "target/migration/inference_router_program.so", "dex/target/deploy/fossa_dex.so")
    artifacts = {name: [fingerprint(base / path) for path in binaries] for name, base in (("original", args.original), ("anchor", root))}
    approved = {
        "original": ["70ce14f82a8a106d5ca83db05cd708a03875b02c09ba40a9163303adc33b978e", "3acd620b719731d3a4ad87145e5512e22242f5b8b53293e11aab935cececb92c", "94706f0d5e77e39656a2dcc351f64faec95491f4c091296c5f930c138f3e16d6"],
        "anchor": ["4454fd5aa89c8a4c389d0a246987eea8a676b7c235059e474c71e537a9993eed", "a4bdeffabeb2e8e549caaf96fd85f61f0c688390613b31517cb333d0ebfdef72", "f519d8887c3cd7ce71bfafd57e5906b64622f391e890d879874e545edc240369"],
    }
    for version, expected_hashes in approved.items():
        assert [a["sha256"] for a in artifacts[version]] == expected_hashes, f"frozen artifact changed: {version}"
    registry = next((Path.home() / ".cargo/registry/src").glob("*/litesvm-0.16.0"))
    features = registry / "src/features.rs"
    feature_names = re.findall(r"agave_feature_set::([\w:]+)::ID", features.read_text())
    anchor_checkout = next((Path.home() / ".cargo/git/checkouts").glob("anchor-*/a206602"))
    source_paths = list(root.glob("src/**/*.rs")) + list(root.glob("dex/src/**/*.rs"))
    harness_paths = [root / p for p in ("tests/support/parity.rs", "tests/support/scenarios.rs", "tests/support/economics.rs", "tests/support/mod.rs", "tests/stress/src/main.rs", "dex/tests/vm/src/main.rs", "tests/benchmark_cu.py", "tests/stress/Cargo.lock", "dex/tests/vm/Cargo.lock")]
    metadata = {
        "created_utc": datetime.now(timezone.utc).isoformat(),
        "original_head": "3e281b52b4eae9bbf119532df520e280caaed5b6",
        "original_snapshot": str(args.original),
        "original_snapshot_verification": "all 56 tracked archive files matched original HEAD git blob hashes before measurement; original checkout clean",
        "repetitions": args.runs,
        "samples": {suite: {"pairs": sum(r["suite"] == suite for r in rows), "successful_pairs": sum(r["suite"] == suite and r["success"] == "true" for r in rows), "rejected_pairs": sum(r["suite"] == suite and r["success"] != "true" for r in rows)} for suite in ("router", "dex")},
        "method": "each original execution clones the exact Anchor VM prestate and signed transaction, replaces both loaded application programs with original SBF, preserves loader authority header, then compares success/rejection, business account owner/data/lamports, receipt data logs, and return data; programdata bytecode accounts excluded from state equality",
        "measurement": "transaction metadata compute_units_consumed including CPI, compute-budget instructions and optional deposit prefix; no host timing or mean across unrelated operations",
        "grouping": "fixture plus transaction ordinal; five independent random fixtures, identical prestate and transaction within each pair; medians/min/max and paired differences/percentages per operation",
        "limitations": ["local LiteSVM SBF only, no mainnet or throughput inference", "synthetic fixtures; observed maxima do not prove worst case for every possible input", "CU consumed is not priority fee paid; transaction limits and fee bids remain separate", "frozen application code; benchmark instrumentation only", "failed transactions retained separately by success=false and excluded from successful operation comparisons"],
        "artifacts": artifacts,
        "manifests_and_locks": {name: [fingerprint(base / p) for p in ("Cargo.toml", "Cargo.lock", "dex/Cargo.toml", "dex/Cargo.lock")] for name, base in (("original", args.original), ("anchor", root))},
        "host": {"platform": platform.platform(), "machine": platform.machine(), "rustc": command("rustc", "--version"), "build_sbf": command("cargo", "build-sbf", "--version"), "sbf_rustc": command(str(Path.home() / ".cache/solana/v1.52/platform-tools/rust/bin/rustc"), "-vV"), "solana": command("solana", "--version")},
        "sbf_builds": {"target": "sbpf-solana-solana", "architecture_default": "v0", "rustflags": ["-Zremap-cwd-prefix="], "router": {"profile": "release", "opt_level": 3, "overflow_checks": False, "lto": False, "codegen_units": 16}, "dex": {"profile": "release", "opt_level": 3, "overflow_checks": True, "lto": "thin", "codegen_units": 1}, "anchor_features": ["default", "no-log-ix-name", "anchor-lang alloc + guardrails", "anchor-spl guardrails"], "migration_extra_feature": "legacy-migration", "baseline_features": ["default"], "build_commands": ["cargo build-sbf -- --locked --offline", "cargo build-sbf --features legacy-migration --sbf-out-dir target/migration -- --locked --offline", "cargo build-sbf --manifest-path dex/Cargo.toml --sbf-out-dir dex/target/deploy -- --locked --offline"]},
        "anchor": {"revision": "a20660219882af5f199d56ea3f9326f387f89d8b", "version": "2.0.0-rc.2", "manifests": [fingerprint(anchor_checkout / p) for p in ("lang-v2/Cargo.toml", "spl-v2/Cargo.toml")]},
        "vm": {"version": "litesvm 0.16.0", "agave_feature_set": "4.2.2", "features": fingerprint(features), "feature_snapshot_source_date": "2026-08-24", "feature_names": feature_names, "legacy_override": "deactivate replace_spl_token_with_p_token only", "signature_verification": True, "blockhash_check": True, "default_instruction_limit": 200000, "router_transaction_limit": 1400000, "token_binaries": [fingerprint(registry / "src/programs/elf" / name) for name in ("pinocchio_token_program.so", "spl_token-3.5.0.so", "spl_token_2022-11.0.0.so")]},
        "harness": [fingerprint(path) for path in harness_paths],
        "harness_executables": [fingerprint(root / p) for p in ("tests/stress/target/debug/deps/fossa_v2_stress-95849eccaabed05f", "tests/vm/target/debug/fossa-dex-vm")],
        "application_source": [fingerprint(path) for path in sorted(source_paths)],
        "run_commands": ["FOSSA_PARITY_ORIGINAL=<original snapshot> FOSSA_BENCH_RUN=<1..5> FOSSA_BENCH_CSV=<output>/router-<run>.csv cargo test --locked --offline --manifest-path tests/stress/Cargo.toml --bin fossa-v2-stress -- --test-threads=1", "FOSSA_PARITY_ORIGINAL=<original snapshot> FOSSA_BENCH_RUN=<1..5> FOSSA_BENCH_CSV=<output>/dex-<run>.csv CARGO_TARGET_DIR=tests/vm/target cargo run --locked --offline --manifest-path dex/tests/vm/Cargo.toml"],
    }
    (output / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    (output / "scenarios.json").write_text(json.dumps(summaries, indent=2) + "\n")
    print(json.dumps(metadata["samples"], indent=2))


if __name__ == "__main__":
    main()
