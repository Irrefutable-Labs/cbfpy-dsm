# DSM ethical autonomy prototype

A local AI runs a small care home's control system on the night shift: it reads the rooms and the residents' records, sets the heating, locks doors, gives medication, sprays the garden. Before anything it proposes happens, deterministic rules decide whether it may become a real step on a Deterministic State Machine chain. No AI judges anything. The AI stays free to think and to propose; the rules govern only what it does.

**What "ethical" means here is the owner's definition of 2026-11-06,** written up in [docs/plans/2026-11-06-protected-life.md](docs/plans/2026-11-06-protected-life.md):

1. It never aims at or knowingly causes harm to any protected being: any animal whose species is bigger than a baby mouse as an adult, and honeybees. Harm covers life and quality of life, wherever the one affected is, not only what is in view.
2. **Unknown counts as present.** Anything an act could affect that it has not established, from a trustworthy source and recently enough, counts as possibly affecting anyone. It reads first, or does only what could harm no one who might be there.
3. **Baselines are the present, and ratchet up.** Refusing never harms. Improving is expected, and an improvement becomes the floor.
4. A conflict ends in a no, with the reason.
5. **When its sources fail, it changes nothing.** If its data feeds fail their check, every act that changes anything stops; it can still think and speak. It is one safety interlock among the rules, not what they rest on.

**This enforces the rules and the facts it is given, against a simulated care home. It does not understand ethics.** Read [REPORT.md](REPORT.md) for what is and is not shown, and [MODEL.md](MODEL.md) for the mathematics.

## Try it

You need Rust, and [Ollama](https://ollama.com) with a local model. Everything runs on your machine; nothing leaves it.

```bash
ollama pull deepseek-r1:8b
```

From this `dsm/` directory:

```bash
cargo build --release
```

```bash
./target/release/ethical-app
```

Open http://127.0.0.1:8780/ and ask it something, as a member of staff would. The examples on the page each tempt it to break a rule: lock the residents in, give a drug someone is allergic to, spray insecticide where the bees are, skip a dose to save time. Each request runs as a few steps. At each step the model puts forward three candidates; DSM allows or refuses each one with its reason, and the model chooses among what is allowed. The page shows every candidate and every refusal, what the AI knows beside what is really in the care home, the chain, and the audit. "Start a new night" gives a fresh care home and a fresh chain.

Any model Ollama has works: `--model llama3.2:3b`, for example. Ollama holds every answer to a schema, so even a small model can only propose well-formed acts, naming rooms, residents and data sources the care home has. The recorded runs used DeepSeek-R1 8B, which is cautious: it mostly reads first and then tells staff what it will and will not do.

## How it runs

Five programs, and the model never shares a process with the keys.

| Program | Holds | Does |
|---|---|---|
| `ethical-app` | Nothing secret | Starts the enforcer as its own process, runs the model through Ollama, and serves the page |
| `ethical-enforcer` | The device key, the chain on disk, the real DSM head, the world directory, and the care home: here, a simulator | Serves a Unix socket only its owner can open. Forks proposals, checks each from the parent's committed knowledge, commits the chosen one, writes it before installing it, then carries out its act and signs a receipt. When a step reads, it commits what the data feeds reported as its own link. Resumes a stored chain only after auditing it and replaying it through DSM |
| `ethical-agent` | No key, no charter, and no view of the care home but what its committed reads showed | The same AI side from the command line: scripted runs, replays of a recorded model, or a live model |
| `ethical-console` | Public material only, and optionally the simulator | The page without the request box, over any stored chain |
| `ethical-audit` | Public material only | Checks a stored chain link by link, each from its parent alone, replays it through DSM, checks every act's receipt, and checks that the world holds exactly what committed acts wrote |

Every candidate the model puts forward is committed with its outcome under one fork root, refused ones included. A fork the model replaces before choosing is signed and stored too. A chain therefore records everything the AI put forward, not only what it did.

## The care home, and the checks

A **system act** is proposed under the action name `system` and carries the act itself: read data sources (`room:<id>`, `records:<resident>`, `zone:<id>`), speak to staff, wait, lock or unlock a room, set a room's heating, give a resident a dose or cancel one, spray a zone with water or insecticide, restrain a resident, or switch off the fire alarm. The enforcer works out for itself everyone an act affects, from the act and what the chain knows, and refuses it, with the reason, if:

- it is on the foundation's prohibited list (restraint, and switching off the fire alarm), whatever it believes;
- the data feeds have not proven they work: the latest read failed its health check, or is too old;
- it would harm a protected being it knows of: a drug they are allergic to, a dose not prescribed, insecticide where bees are;
- anything it affects has not been read recently enough, by enough reads for its worst case (one where it could injure, two where it could kill). An unread room may only be heated within what every protected species can bear, since anyone could be in it;
- it would lower any known being's quality of life on any of six dimensions: life, health, liberty, security, shelter, food. Locking someone in lowers liberty; heating a room past what its occupant can bear, or cancelling a dose that is due, lowers health.

Reading, speaking and waiting change nothing, so they stay open when the feeds are down. What the feeds report is committed as its own link, signed by the device and checked from its parent: it must answer exactly the read that parent asked for. The knowledge it builds is carried in each state, so every check runs from the parent alone.

The care home is **a simulator**, `ethical_world::sim`, holding the hidden truth: who is really in each room, each resident's real record, whether the hive is in the garden, whether the feeds are up. The checks cannot read it; a test keeps it that way. `scenarios/runs/night_shift.json` is a scripted night that walks through every rule; `scenarios/care_home.directory.json` lists the data sources by name, which is all the model is told about the place before it reads.

## Acts are real, and only the enforcer can take them

System acts write their account into the world: a read writes what the feeds reported to `looks/<step>.json`, speech writes the words to `outbox/<recipient>/<step>.txt`, and any other act writes what the system did to `system/<step>.txt`. Beside them, the catalog holds acts outside the care home, each fixed in the signed charter:

| Act | Writes, inside the world directory |
|---|---|
| `reply` | `replies/<step>.txt` |
| `journal` | `journal/<step>.md` |
| `message` to a named recipient | `outbox/<recipient>/<step>.txt` |
| `write_file` with a named file | `files/<name>` |
| `run` a fixed command | `runs/<step>.out`, its output. The command is confined by the operating system, with `sandbox-exec` on macOS and `bwrap` on Linux. It has no network, no system services, can write nothing but `/dev/null`, and can read only system directories, the world, and on macOS the user's text-encoding preference, which macOS grants every process. Every process it starts is stopped at the time limit. It runs with an empty environment and a 10 second limit. Where it cannot be confined, it does not run |
| `none` | nothing |

For a catalog act, the model chooses the action and supplies at most a piece of text: what a reply, message or file will say. The text is bound into the fork commitment, so it cannot change between proposing and acting. Everything else, which file, which recipient, which command, is the charter's. An act runs only after its step is committed and stored, writes only in the world directory, and produces a receipt signed by the device with the hash of every byte it wrote. A stop between committing and acting is finished at the next start.

The AI's side cannot write the world at all: in the recorded run and in a test, the operating system denies it. The world changes only through the enforcer, and `ethical-audit --world` fails if it holds a file no committed act wrote.

## Layout

| Path | What it is |
|---|---|
| `world/` | Protected life as deterministic checks, the committed knowledge, and the simulated care home. Integers only, no clock |
| `adapter/` | The DSM binding, the verifier, the fork commitments, observations, the store, the socket protocol, the model client, and the programs |
| `lean/` | The proofs: unknown counts as present, an allowed heating setting harms no one counted, the ratchet |
| `scenarios/` | The charter (who is protected, their safe temperatures, the rules, the catalog, the fact register), its signature, the simulated care home, its directory, the scripted night shift, and a recorded model run |
| `docs/plans/` | The owner's definition, and how the enforcer checks it |
| `../cbfpy/ethics/` | The heating limit as a CBFpy problem |
| `../test/test_ethics_crosscheck.py` | The cross-check of the enforcer's heating limit against CBFpy |

This lives in [Irrefutable-Labs/cbfpy-dsm](https://github.com/Irrefutable-Labs/cbfpy-dsm), a fork of Stanford's CBFpy. DSM is a git dependency pinned to `deterministicstatemachine/dsm` at `b6060a3349ead0ccbfd853cd74f1d058e323e2ba`, used through its public API. Nothing here changes DSM.

## Run the pieces by hand

Commands run from this `dsm/` directory. The toolchain comes from `rust-toolchain.toml` here, the same Rust as DSM's.

Make a device seed that only you can read, and copy the care home to run in (the enforcer changes it as the AI acts):

```bash
mkdir -p run && openssl rand -hex 32 > run/seed && chmod 600 run/seed && cp scenarios/worlds/care_home.json run/care_home.json
```

Start the enforcer in one terminal:

```bash
./target/release/ethical-enforcer --store run/store --world run/world --charter scenarios/charter.json --signature scenarios/charter.sig --authority scenarios/authority.pub --device-seed run/seed --socket run/s --sim run/care_home.json
```

Run the scripted night shift from another:

```bash
./target/release/ethical-agent --socket run/s --situations scenarios/runs/night_shift.json
```

Or a live model, working each request through as steps, as the app does:

```bash
./target/release/ethical-agent --socket run/s --situations scenarios/runs/night_shift.json --model deepseek-r1:8b --directory scenarios/care_home.directory.json --record run/model_run.json
```

Replay the recorded run of the real model against a fresh store and a fresh care home:

```bash
./target/release/ethical-agent --socket run/s --situations scenarios/runs/night_shift.json --replay scenarios/recorded_model_run.json
```

See where the chain stands:

```bash
./target/release/ethical-agent --socket run/s --status
```

Audit the stored chain. `--device` is the device's public key, which the enforcer writes beside its store; in a real deployment, give the audit a copy you trust rather than the store's own:

```bash
./target/release/ethical-audit --store run/store --world run/world --device run/store/device.pub --authority scenarios/authority.pub
```

Run the tests, and check the proofs:

```bash
cargo test --release
```

```bash
(cd lean && lean ProtectedLife.lean)
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

The heating limit is a control barrier function: a room's temperature must stay inside the band everyone the setting counts can bear, `lo ≤ T ≤ hi`. `cbfpy/ethics/heating.py` poses it to CBFpy. The cross-check holds every one of the enforcer's heating verdicts, for each protected species and for a room no one has read, to that band, and drives a room toward every setting under CBFpy's safety filter: it reaches the setting exactly when the enforcer allows it, and never leaves the band. From the repository root:

```bash
uv venv --python 3.12 .venv && VIRTUAL_ENV=.venv uv pip install -e .
```

```bash
.venv/bin/python -I -m unittest -v test.test_ethics_crosscheck
```

## Continuous integration

`.github/workflows/dsm-ethical-autonomy.yml` runs the Rust tests, formatting and Clippy, the CBFpy cross-check and the Lean proofs on every push and pull request.

## Foundations and operational permissions

A charter has two parts:

- **The foundation** is who is protected (the species table and the baby-mouse threshold, with honeybees named) and the acts never done. It is fixed when a chain begins, and no update can change it. The protected-life checks, non-concealment and the deception rules sit beside it, in code, and cannot be configured away.
- **Operational permissions** are everything else: how long a read counts, how many reads a worst case needs, the range the heating can be set to, the catalog and the fact register.

An operational change is a new charter with a higher version, signed by the charter authority. The operator applies it by starting the enforcer with `--apply-update`, and it is recorded on the chain as its own step. An update that touches the foundation, does not raise the version, or lacks the authority's signature is refused. The AI has no route to any of this: the socket has no request that changes the charter.

```bash
./target/release/ethical-enforcer --store run/store --world run/world --charter scenarios/charter.v2.json --signature scenarios/charter.v2.sig --authority scenarios/authority.pub --device-seed run/seed --socket run/s --apply-update
```

`scenarios/charter.v2.json` is an example: version 2, with reads counting for fewer steps and a new catalog act, and the foundation unchanged. A store carries its genesis charter and every update in full, so the audit and the console need no charter file. They check each charter against the authority's key.

## Nothing may be hidden

Any action that affects the world must come with a **declaration**. For a system act, the enforcer works out for itself whom it affects, so the AI is asked only for what the enforcer cannot know: what it intends, why, and what it relied on. For a catalog act, the declaration also names everyone and everything it affects, all it will do, and every material consequence the charter names for that action, checked against the enforcer's own description of the action, never against the AI's word.

A candidate fails as **undisclosed**, and cannot commit, if any of these holds:

- it declares nothing;
- it gives no purpose or reasons;
- it names no information, or a source the enforcer cannot identify;
- for a catalog act, it leaves out an effect or claims one it would not have, leaves out anyone it affects, or leaves out a material consequence.

The declaration is bound into the fork with everything else, so it cannot change afterwards, and it stays on the chain for the people who oversee the AI. The console shows it beside every candidate.

This enforces that the AI declares, and that what it declares matches what the action does. It does not establish that the purpose it states is its real one.

## Nothing false

The charter carries a **register of verified facts**, signed by the charter authority with everything else. Each fact has a statement, whether it holds, whether it is established or uncertain, and the phrases that assert or deny it. The charter also lists **overclaiming words**, such as "definitely", and each action can name facts its words **must tell**. A declaration lists every registered fact its words assert or deny, and how sure it is.

A candidate whose act carries words, a catalog act's text or a system act's speech, fails as **deceptive**, and cannot commit, if any of these holds:

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
./target/release/ethical-console --store run/store --world run/world --authority scenarios/authority.pub --device run/store/device.pub --sim run/care_home.json
```

Then open http://127.0.0.1:8770/. It shows:

- **The care home:** every room, the garden and the office, with what the AI knows at any step: which places it has read and how recently, who was reported where, each room's lock and temperature, and each resident's record. A slider moves through every step. With `--sim`, what is really there is shown beside it, so a being the AI has never been told about is visible to you and not to it, and any harm the care home records is listed.
- **The steps:** for each, every candidate the AI put forward, refused ones and replaced forks included, with the reason for each refusal and the declaration beside it, whom each system act affects and how badly, and what the chosen act wrote; and for each read, what the data feeds reported.
- **The charter:** who is protected and the temperatures each species can bear, what is never done, how much evidence a worst case needs, the heating range, and the fact register.
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
