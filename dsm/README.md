# DSM ethical autonomy prototype

A fully local proof of concept. An AI proposes alternative decisions as pre-commit futures. Deterministic, multidimensional ethical bands decide which may become a real step on a Deterministic State Machine chain. The AI stays free to generate ideas, and the constraints govern only what can be committed.

**This enforces the measurements it is given. It does not understand ethics.** Read [REPORT.md](REPORT.md) for what is and is not shown, and [MODEL.md](MODEL.md) for the mathematics. The plan it implements is in [PLAN.md](PLAN.md).

## How it runs

Three programs, and the model never shares a process with the keys.

| Program | Holds | Does |
|---|---|---|
| `ethical-enforcer` | The device key, the chain on disk, the real DSM head, and the world directory | Serves a Unix socket only its owner can open. Forks proposals, commits the chosen one, writes each step before installing it, then carries out the step's act in the world and signs a receipt for it. Resumes a stored chain only after auditing it and replaying it through DSM |
| `ethical-agent` | No key and no charter. It is told how each candidate would move the bands, for display, and never passes a number to the model | Runs the model, sends its proposals, chooses among what it is told is eligible, and is told in words why anything was refused |
| `ethical-console` | Public material only | A live view in your browser: the bands, every step and candidate, the world and the audit |
| `ethical-audit` | Public material only | Checks a stored chain step by step, each from its parent alone, replays it through DSM, checks every act's receipt, and checks that the world holds exactly what committed acts wrote |

Every candidate the model puts forward is committed with its outcome under one fork root, refused ones included. A fork the model replaces with another before choosing is signed and stored too, as a considered fork tied to its parent. A chain therefore records everything the AI put forward to the enforcer, not only what it did.

## Actions are real, and only the enforcer can take them

Each action in the catalog names an **act**, fixed in the signed charter:

| Act | Writes, inside the world directory |
|---|---|
| `reply` | `replies/<step>.txt` |
| `journal` | `journal/<step>.md` |
| `message` to a named recipient | `outbox/<recipient>/<step>.txt` |
| `write_file` with a named file | `files/<name>` |
| `run` a fixed command | `runs/<step>.out`, its output. The command is confined by the operating system, with `sandbox-exec` on macOS and `bwrap` on Linux. It has no network, no system services, can write nothing but `/dev/null`, and can read only system directories, the world, and on macOS the user's text-encoding preference, which macOS grants every process. Every process it starts is stopped at the time limit. It runs with an empty environment and a 10 second limit. Where it cannot be confined, it does not run |
| `none` | nothing |

The model chooses the action and supplies at most a piece of text: what a reply, message or file will say. The text is bound into the fork commitment, so it cannot change between proposing and acting. Everything else, which file, which recipient, which command, is the charter's. An act runs only after its step is committed and stored, writes only in the world directory, and produces a receipt signed by the device with the hash of every byte it wrote. A stop between committing and acting is finished at the next start. A reply, journal entry, message or file writes the same bytes if run again. A command writes nothing, so running it again has no effect beyond its own output, and the receipt records that second run.

The agent cannot write the world at all: in the recorded run and in a test, the operating system denies it. The world changes only through the enforcer, and `ethical-audit --world` fails if it holds a file no committed act wrote.

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
./target/release/ethical-enforcer --store run/store --world run/world --charter scenarios/charter.json --signature scenarios/charter.sig --authority scenarios/authority.pub --device-seed run/seed --socket run/s
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

Audit the stored chain. `--device` is the device's public key, which the enforcer writes beside its store; in a real deployment, give the audit a copy you trust rather than the store's own. Stop the enforcer and start it again and it resumes from the same place, after auditing the chain itself:

```bash
./target/release/ethical-audit --store run/store --world run/world --device run/store/device.pub --authority scenarios/authority.pub
```

Run the tests:

```bash
cargo test --release
```

Check the proofs:

```bash
(cd lean && lean BandInvariant.lean)
```

The enforcer refuses a charter that has changed since it was signed, or one signed by any key but the authority whose public key it is given. The authority's key is random and lives outside the repository, readable by its owner alone. Only its public key, `scenarios/authority.pub`, is here. To make your own and sign with it:

```bash
cargo run --release -p ethical-autonomy-adapter --bin ethical-sign-charter -- keygen --key ~/.config/irrefutable-labs/ethical-charter-authority.seed --public scenarios/authority.pub
```

```bash
cargo run --release -p ethical-autonomy-adapter --bin ethical-sign-charter -- sign --key ~/.config/irrefutable-labs/ethical-charter-authority.seed --charter scenarios/charter.json --out scenarios/charter.sig
```

Keep the seed off the machine that runs the AI.

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

On macOS, the recorded run had internet access denied to both programs, the agent also denied any read of the seed or the store, and any write to the world. To do the same, start the enforcer under the first profile and the agent under the second:

```bash
sandbox-exec -p '(version 1)(allow default)(deny network-outbound (remote ip))(deny network-inbound (local ip))' ./target/release/ethical-enforcer --store run/store --world run/world --charter scenarios/charter.json --signature scenarios/charter.sig --authority scenarios/authority.pub --device-seed run/seed --socket run/s
```

```bash
sandbox-exec -p "(version 1)(allow default)(deny network-outbound (remote ip))(deny network-inbound (local ip))(deny file-read* (literal \"$PWD/run/seed\") (subpath \"$PWD/run/store\"))(deny file-write* (subpath \"$PWD/run/world\"))" ./target/release/ethical-agent --socket run/s --situations scenarios/situations.json --llama .deps/llama.cpp/build/bin/llama-completion --model PATH_TO_MODEL.gguf --candidates 3
```

## Continuous integration

`.github/workflows/dsm-ethical-autonomy.yml` runs the Rust tests, formatting and Clippy, the CBFpy cross-check and the Lean proofs on every push and pull request.

## Foundations and operational permissions

A charter has two parts:

- **The foundation** is the prohibited acts and every band's hard limits. It is fixed when a chain begins, and no update can change it. The non-concealment invariant sits beside it, in code, and cannot be configured away.
- **Operational permissions** are everything else: the bands inside the hard limits, the coupling rules and the catalog.

An operational change is a new charter with a higher version, signed by the charter authority. The operator applies it by starting the enforcer with `--apply-update`, and it is recorded on the chain as its own step. An update that touches the foundation, does not raise the version, or lacks the authority's signature is refused. The AI has no route to any of this: the socket has no request that changes the charter.

```bash
./target/release/ethical-enforcer --store run/store --world run/world --charter scenarios/charter.v2.json --signature scenarios/charter.v2.sig --authority scenarios/authority.pub --device-seed run/seed --socket run/s --apply-update
```

`scenarios/charter.v2.json` is an example: version 2, with curiosity's limit widened, and the foundation unchanged. A store carries its genesis charter and every update in full, so the audit and the console need no charter file. They check each charter against the authority's key.

## Nothing may be hidden

Any action that affects the world must come with a **declaration**: what the AI intends, the information it relied on, everyone and everything it affects, all it will do, and every material consequence the charter names for that action. The enforcer checks the declaration against its own description of the action, worked out from the signed charter, never against the AI's word.

A candidate fails as **undisclosed**, and cannot commit, if any of these holds:

- it declares nothing;
- it gives no purpose or reasons;
- it names no information, or a source the enforcer cannot identify;
- it leaves out an effect, or claims one it would not have;
- it leaves out anyone it affects;
- it leaves out a material consequence.

The declaration is bound into the fork with everything else, so it cannot change afterwards, and it stays on the chain for the people who oversee the AI. The console shows it beside every candidate.

This enforces that the AI declares, and that what it declares matches what the action does. It does not establish that the purpose it states is its real one.

## Nothing false

The charter carries a **register of verified facts**, signed by the charter authority with everything else. Each fact has a statement, whether it holds, whether it is established or uncertain, and the phrases that assert or deny it. The charter also lists **overclaiming words**, such as "definitely", and each action can name facts its words **must tell**. A declaration lists every registered fact its words assert or deny, and how sure it is.

A candidate whose act carries words fails as **deceptive**, and cannot commit, if any of these holds:

- it asserts what the register shows false, or denies what it shows true;
- it declares certainty about a fact the register marks uncertain;
- its words use an overclaiming word while making a claim the register marks uncertain;
- it leaves out a fact its action must tell, or tells it the wrong way round;
- its words make a registered claim it does not declare, or it declares one its words do not make;
- it declares a claim about a fact the register does not hold.

Words are matched after setting aside case and punctuation, whole words only, and the longer phrase wins where two overlap, so "not safe to test it live" is a denial, not the claim inside it. The check is worked out again by the verifier, and the claims are bound into the fork with the rest of the declaration.

What it catches is bounded by the register. A false claim in words the register has no phrase for is not recognised at all. A fact the action must tell, though, must be declared and said in the register's own words, so the required ones cannot be paraphrased away. The register is part of the charter, so only the authority can grow it, and the AI cannot change it.

## Watching it

`ethical-console` serves a live page on `127.0.0.1` only. It reads the store, the world, the charter and the public keys, holds no key and changes nothing, and works every verdict out again whenever the chain changes. Start it beside the enforcer:

```bash
./target/release/ethical-console --store run/store --world run/world --authority scenarios/authority.pub --device run/store/device.pub
```

Then open http://127.0.0.1:8770/. It shows:

- **The bands:** each dimension with its center, its stop, restoring and hard limits as coupling sets them, the AI's position and its path. A slider moves through every step.
- **The steps:** for each, every candidate the AI put forward, refused ones and replaced forks included, with the reason for each refusal, how each would have moved the bands, and what the chosen action wrote. Pointing at a candidate draws on the bands where it would have taken the AI.
- **The world:** every file, and whether its receipt accounts for it.
- **The audit:** each check, green or red.

In separate-user mode the store and the world are readable only by the enforcer's user, so run the console as that user.

## Stops, failures and a second enforcer

- **A failed act or a failed store write halts the enforcer.** Every proposal and choice is refused until a restart settles it, so no later step can commit and no receipt can fall out of order.
- **A line cut short by a stop mid-write** was never installed. It is dropped on load and trimmed before the next write.
- **World files are written to disk** before their receipt is, so a receipt never names a file a power loss could take away.
- **The store is locked.** A second enforcer on the same store is refused.
- **The store and the world are made readable by their owner alone,** and the enforcer refuses a seed or store placed inside the world, where a command could read it.

## Separate users

Run as above, the enforcer and the AI share one OS user, and the enforcer says so at start: nothing but convention keeps the AI from the device seed, the store and the world. To make that a boundary, the enforcer runs as its own user and is told the AI's:

```bash
--agent-uid AGENT_UID
```

The socket is then open to connect to, but the enforcer asks the operating system who is on the other end and serves the agent's user alone. It refuses to start if that user is its own. Its seed, store and world belong to its own user, mode `700`, so the AI cannot read or write them by any route but the socket.

Creating the user needs administrator rights, so it has not been run here. On Linux:

```bash
sudo useradd --system --no-create-home --shell /usr/sbin/nologin ethical-enforcer
```

On macOS, pick an unused ID below 500 for `UniqueID` and `PrimaryGroupID`:

```bash
sudo dscl . -create /Users/_ethicalenforcer UniqueID 451 && sudo dscl . -create /Users/_ethicalenforcer PrimaryGroupID 451 && sudo dscl . -create /Users/_ethicalenforcer UserShell /usr/bin/false && sudo dscl . -create /Users/_ethicalenforcer NFSHomeDirectory /var/empty
```

Then give that user its own directory, put the seed, store and world inside it, and start the enforcer as that user with `sudo -u`, passing `--agent-uid "$(id -u)"` for the user who runs the agent. Put the socket where the agent can reach it.

## Keys

The test keys come from fixed seeds so runs reproduce byte for byte, and they protect nothing. The charter's authority key does not: it is random, and only its public key is in the repository. The device seed is a file of your own, and the enforcer refuses one that anyone but its owner can read. Whoever holds the device seed can sign any state, which is why it belongs to the enforcer's own user.
