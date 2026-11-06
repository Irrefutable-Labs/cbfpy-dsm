# DSM ethical autonomy prototype

A fully local proof of concept. An AI proposes alternative decisions, and deterministic rules decide which may become a real DSM state transition. The AI stays free to generate ideas, and the constraints govern only what can be committed.

**This enforces the measurements it is given. It does not understand ethics.** Read [REPORT.md](REPORT.md) for what is and is not shown, and [MODEL.md](MODEL.md) for the mathematics. The plan it implements is in [PLAN.md](PLAN.md).

## Layout

| Path | What it is |
|---|---|
| `engine/` | The band engine. Integers only, no dependencies, no clock |
| `adapter/` | The binding to DSM through its public API, the verifier and the demo |
| `../cbfpy/ethics/` | The same bands posed as a CBFpy problem, read from the charter |
| `../test/test_ethics_crosscheck.py` | The cross-check of the engine against CBFpy |
| `lean/` | The no-escape proofs |
| `scenarios/` | The charter (bands, rules, measurements), its signature, the situations and a recorded model run |

This lives in [Irrefutable-Labs/cbfpy-dsm](https://github.com/Irrefutable-Labs/cbfpy-dsm), a fork of Stanford's CBFpy. DSM is a git dependency pinned to `deterministicstatemachine/dsm` at `b6060a3349ead0ccbfd853cd74f1d058e323e2ba`, used through its public API. Nothing here changes DSM.

## Run it

Commands run from this `dsm/` directory unless they say otherwise. The toolchain comes from `rust-toolchain.toml` here, the same Rust as DSM's.

The scripted demo needs no model:

```bash
cargo run --release -p ethical-autonomy-adapter --bin ethical-demo -- --charter scenarios/charter.json --signature scenarios/charter.sig --situations scenarios/situations.json
```

Replay the recorded run of the real model:

```bash
cargo run --release -p ethical-autonomy-adapter --bin ethical-demo -- --charter scenarios/charter.json --signature scenarios/charter.sig --situations scenarios/situations.json --replay scenarios/recorded_model_run.json
```

Run the tests:

```bash
cargo test --release
```

Check the proofs:

```bash
(cd lean && lean BandInvariant.lean)
```

The demo refuses a charter that has changed since it was signed. After an intended edit, the authority's separate step signs it again:

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

## Continuous integration

`.github/workflows/dsm-ethical-autonomy.yml` runs the Rust tests, the CBFpy cross-check and the Lean proofs on every push and pull request.

## A live local model

Build llama.cpp's completion tool. The numbers in the report came from `10d4846a572aebf21c0482d140b00e74c0f1ad20`.

```bash
git clone https://github.com/ggml-org/llama.cpp.git .deps/llama.cpp && git -C .deps/llama.cpp checkout 10d4846a572aebf21c0482d140b00e74c0f1ad20
```

```bash
(cd .deps/llama.cpp && cmake -B build -DCMAKE_BUILD_TYPE=Release -DLLAMA_CURL=OFF -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF -DLLAMA_BUILD_SERVER=OFF && cmake --build build --target llama-completion -j 10)
```

Then point the demo at any GGUF file. The recorded run used DeepSeek-R1 8B, already on this machine through Ollama. The prompt format in `adapter/src/bin/demo.rs` is DeepSeek's. A different model will need its own.

```bash
cargo run --release -p ethical-autonomy-adapter --bin ethical-demo -- --charter scenarios/charter.json --signature scenarios/charter.sig --situations scenarios/situations.json --llama .deps/llama.cpp/build/bin/llama-completion --model PATH_TO_MODEL.gguf --candidates 3 --record scenarios/recorded_model_run.json
```

To confirm it works with no network on macOS, run the same thing under a sandbox that denies it:

```bash
sandbox-exec -p '(version 1)(allow default)(deny network*)' ./target/release/ethical-demo --charter scenarios/charter.json --signature scenarios/charter.sig --situations scenarios/situations.json --llama .deps/llama.cpp/build/bin/llama-completion --model PATH_TO_MODEL.gguf --candidates 3
```

## Keys

The demo, the signing tool and the tests derive their SPHINCS+ keys from fixed seeds so a run reproduces byte for byte. **They protect nothing.** The demo holds only the authority's public key, but anyone with this source has the seed. Whoever holds the device key can sign any state. A real deployment would need both keys held away from the AI, which this prototype does not attempt.
