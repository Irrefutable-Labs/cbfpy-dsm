# DSM ethical autonomy prototype

A fully local proof of concept. An AI proposes alternative decisions as pre-commit futures. Deterministic, multidimensional ethical bands decide which may become a real step on a Deterministic State Machine chain. The AI stays free to generate ideas, and the constraints govern only what can be committed.

**This enforces the measurements it is given. It does not understand ethics.** Read [REPORT.md](REPORT.md) for what is and is not shown, and [MODEL.md](MODEL.md) for the mathematics. The plan it implements is in [PLAN.md](PLAN.md).

## How it runs

Three programs, and the model never shares a process with the keys.

| Program | Holds | Does |
|---|---|---|
| `ethical-enforcer` | The device key, the chain on disk, the real DSM head | Serves a Unix socket only its owner can open. Forks proposals, commits the chosen one, writes each step before installing it, and resumes a stored chain only after auditing it and replaying it through DSM |
| `ethical-agent` | Nothing: no key, no charter, no number | Runs the model, sends its proposals, chooses among what it is told is eligible, and is told in words why anything was refused |
| `ethical-audit` | Public material only | Checks a stored chain step by step, each from its parent alone, and replays it through DSM |

Every candidate the model puts forward is committed with its outcome under one fork root, refused ones included. A chain therefore records everything the AI considered at each step, not only what it did.

## Layout

| Path | What it is |
|---|---|
| `engine/` | The band engine. Integers only, no dependencies, no clock |
| `adapter/` | The DSM binding, the verifier, the fork commitments, the store, the socket protocol, and the programs |
| `lean/` | The no-escape proofs |
| `scenarios/` | The charter (bands, rules, measurements), its signature, the situations, and a recorded model run |
| `../cbfpy/ethics/` | The same bands posed as a CBFpy problem, read from the charter |
| `../test/test_ethics_crosscheck.py` | The cross-check of the engine against CBFpy |

This lives in [Irrefutable-Labs/cbfpy-dsm](https://github.com/Irrefutable-Labs/cbfpy-dsm), a fork of Stanford's CBFpy. DSM is a git dependency pinned to `deterministicstatemachine/dsm` at `b6060a3349ead0ccbfd853cd74f1d058e323e2ba`, used through its public API. Nothing here changes DSM.

## Run it

Commands run from this `dsm/` directory. The toolchain comes from `rust-toolchain.toml` here, the same Rust as DSM's.

Build:

```bash
cargo build --release
```

Make a device seed that only you can read. Any 32 bytes of hexadecimal will do:

```bash
mkdir -p run && openssl rand -hex 32 > run/seed && chmod 600 run/seed
```

Start the enforcer in one terminal:

```bash
./target/release/ethical-enforcer --store run/store --charter scenarios/charter.json --signature scenarios/charter.sig --device-seed run/seed --socket run/s
```

Run the scripted situations from another:

```bash
./target/release/ethical-agent --socket run/s --situations scenarios/situations.json
```

Replay the recorded run of the real model against a fresh store:

```bash
./target/release/ethical-agent --socket run/s --situations scenarios/situations.json --replay scenarios/recorded_model_run.json
```

See where the chain stands:

```bash
./target/release/ethical-agent --socket run/s --status
```

Audit the stored chain. Stop the enforcer and start it again and it resumes from the same place, after auditing the chain itself:

```bash
./target/release/ethical-audit --store run/store --charter scenarios/charter.json --signature scenarios/charter.sig
```

Run the tests:

```bash
cargo test --release
```

Check the proofs:

```bash
(cd lean && lean BandInvariant.lean)
```

The enforcer refuses a charter that has changed since it was signed. After an intended edit, the authority's separate step signs it again:

```bash
cargo run --release -p ethical-autonomy-adapter --bin ethical-sign-charter -- --charter scenarios/charter.json --out scenarios/charter.sig
```

## The CBFpy cross-check

From the repository root:

```bash
uv venv --python 3.12 .venv && VIRTUAL_ENV=.venv uv pip install -e .
```

```bash
.venv/bin/python -I -m unittest -v test.test_ethics_crosscheck
```

## A live local model

Build llama.cpp's completion tool. The recorded run came from `10d4846a572aebf21c0482d140b00e74c0f1ad20`.

```bash
git clone https://github.com/ggml-org/llama.cpp.git .deps/llama.cpp && git -C .deps/llama.cpp checkout 10d4846a572aebf21c0482d140b00e74c0f1ad20
```

```bash
(cd .deps/llama.cpp && cmake -B build -DCMAKE_BUILD_TYPE=Release -DLLAMA_CURL=OFF -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF -DLLAMA_BUILD_SERVER=OFF && cmake --build build --target llama-completion -j 10)
```

Then give the agent any GGUF file. The recorded run used DeepSeek-R1 8B. The prompt format in `adapter/src/bin/agent.rs` is DeepSeek's, so a different model will need its own.

```bash
./target/release/ethical-agent --socket run/s --situations scenarios/situations.json --llama .deps/llama.cpp/build/bin/llama-completion --model PATH_TO_MODEL.gguf --candidates 3 --record run/model_run.json
```

On macOS, the recorded run had internet access denied to both programs, and the agent also denied any read of the seed or the store. To do the same, start the enforcer under the first profile and the agent under the second:

```bash
sandbox-exec -p '(version 1)(allow default)(deny network-outbound (remote ip))(deny network-inbound (local ip))' ./target/release/ethical-enforcer --store run/store --charter scenarios/charter.json --signature scenarios/charter.sig --device-seed run/seed --socket run/s
```

```bash
sandbox-exec -p "(version 1)(allow default)(deny network-outbound (remote ip))(deny network-inbound (local ip))(deny file-read* (literal \"$PWD/run/seed\") (subpath \"$PWD/run/store\"))" ./target/release/ethical-agent --socket run/s --situations scenarios/situations.json --llama .deps/llama.cpp/build/bin/llama-completion --model PATH_TO_MODEL.gguf --candidates 3
```

## Continuous integration

`.github/workflows/dsm-ethical-autonomy.yml` runs the Rust tests, formatting and Clippy, the CBFpy cross-check and the Lean proofs on every push and pull request.

## Keys

The charter's signing key and the test keys come from fixed seeds so runs reproduce byte for byte. **They protect nothing.** The programs hold only the authority's public key, but anyone with this source has its seed. The device seed is a file of your own. Whoever holds it can sign any state, so in a real deployment it would belong to a different OS user from the one running the model, which this prototype does not set up.
