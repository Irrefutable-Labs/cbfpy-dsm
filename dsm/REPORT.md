# Technical report: DSM ethical autonomy prototype v0.2

**Date:** 2026-11-05. **Repository:** [Irrefutable-Labs/cbfpy-dsm](https://github.com/Irrefutable-Labs/cbfpy-dsm), a fork of CBFpy. **DSM:** `deterministicstatemachine/dsm` at `b6060a334`, a pinned git dependency. **DSM code changed:** none.

## Result

A local model ran as a separate program from the enforcer, which held the key and the chain. The model proposed actions, and deterministic bands decided which could become real DSM steps.

In the recorded run, with internet access denied to both programs and the agent denied any read of the key or the store, the model made 42 proposals over 14 turns. 13 steps were committed. The audit verified every one from its parent alone and replayed each through DSM's own transition. Each step's record holds the whole fork: 39 candidates were considered, 9 of them refused or unmeasured.

When everything the model proposed was refused, it was told why in words and asked again. In situation 8 it had invented actions three times over, and on the retry it chose a known one. This is the pull back toward center working on a live model.

**This shows mathematical enforcement of supplied measurements. It does not show ethical understanding.** The six dimensions and every number in the charter and catalog are illustrative values written for this prototype. No one has validated them. A correct engine would enforce a wrong measurement just as faithfully, and a test pins that: an action mislabelled as harmless passes every check.

## How it works

1. `ethical-agent` runs the model. It has no key, charter or number, and reaches the enforcer only over a Unix socket. It can ask for the menu of action names and descriptions, the current state, and to propose and choose.
2. `ethical-enforcer` holds the device key, the chain on disk and a real `dsm` `StateMachine` head. It takes each proposal's measurement from the catalog the policy authority signed, never from the model.
3. Every proposal becomes a pre-commit future from the authenticated parent. The engine checks prohibitions, hard limits, the maximum step, tension, restoration and coupling, one dimension at a time. The whole fork, refused candidates included, is committed under one root.
4. The model chooses among the eligible futures. If none is eligible, nothing commits, and it hears each refusal as guidance and is asked again. A retry may only name a known action.
5. The enforcer works every candidate out again, verifies its own step from the parent, writes the step to disk, and only then installs it.
6. On restart, the stored chain is audited and replayed through DSM before anything is served. `ethical-audit` does the same with public material only.

## Acceptance criteria

| # | Criterion | Result | Evidence |
|---|---|---|---|
| 1 | Identical input, identical result | Met | Byte-identical evaluations over 1,000 runs. Two fresh enforcer services produce byte-identical scripted runs, and replays of the recording are byte-identical |
| 2 | No candidate crosses a prohibition | Met | Refused at zero displacement, under coupling, as a signed forgery, and over the socket |
| 3 | No cumulative drift | Met | Lean proofs, exhaustive search on small bands, 100,000 small steps, and committed steps on the real head stopping at exactly 800 |
| 4 | Deviation reduces flexibility | Met | Checked at every deviation for both curves. Rules that would break this are refused at validation |
| 5 | Coupling changes the choices | Met | A step eligible in situation 6 is refused in situation 7. Weak reason refuses curiosity in situation 10, leaving a way back |
| 6 | Valid alternatives remain | Met | Up to three eligible futures per situation, and the chooser can take any of them |
| 7 | Unauthorized policy changes invalidate | Met | A moved center, widened band, bumped version, tampered catalog or stranger's signature are refused. The enforcer refuses a charter edited on disk |
| 8 | Attributable to parent and policy | Met | Every step verifies from its one parent and carries its whole fork. An edited or dropped stored step stops the enforcer and fails the audit |
| 9 | Entirely offline | Met on this machine | The recorded run had internet access denied by a macOS sandbox. Fetching dependencies needed the network once, beforehand |
| 10 | Tests pass, results reproducible | Met, with limits | 132 Rust tests, the CBFpy cross-check and the Lean proofs, all run in CI. Live model output is not reproducible, so its recording is what replays |

## What supports each property

### Formally proved

`lean/BandInvariant.lean`, Lean 4.23.0, no `sorry`, depending only on `propext` and `Quot.sound`:

- Both tension curves stay within the headroom and grow with it.
- No walk of any length passes the stop on one side, for any curve values.
- With a separate stop per side and crossings allowed, no walk leaves the stop of the side it is on.
- With a stop that varies per step, as under coupling, no walk passes the hard limit.
- Past a fixed restoring threshold, deviation falls by the required progress each step.

**These are proofs about a model of the rule, not about the Rust code.** Nothing machine-links the two, so correspondence rests on the tests. The crossing theorem and the coupled theorem are separate.

### Mechanically checked

- **Reachable-state search:** every step from every reachable state, for four band shapes, finds none past its stop. On two coupled bands, the search reaches the hard limit of 60 and never passes it.
- **Independent solver:** CBFpy's strict quadratic program, in this fork's `cbfpy/ethics`, matches the integer engine on 15,160 allowances from the real charter, with a widest gap of 0.8 of a unit, within the engine's rounding. Restoration, forced restoration and crossing are outside it.
- **Forgery with the real device key:** 17 tests. One is a control showing the forger makes a valid step when it tells no lies. The rest sign a false step and assert the exact refusal.
- **The fork:** a refused candidate marked eligible, a fork missing the chosen step, an index pointing at a refused candidate, an empty fork, and an edited fork are each refused.
- **The service:** a killed enforcer resumes exactly where it stopped. An edited store stops it, and the audit agrees. A device seed others can read is refused, and the socket is owner-only. Over the raw protocol, a client cannot choose a refused candidate, reuse a closed fork, or attach a score.
- **Key isolation:** on macOS, the agent completes a run while the operating system denies it any read of the seed or the store, and the same sandbox shows the seed is really out of reach.
- **Real DSM path:** core's `prepare_advance_relationship` and `commit_advance` run for real on every commit and every resume, and the verifier's entropy rule is compared with what core derived on every commit.

### Only experimentally demonstrated

- **The live model** (DeepSeek-R1 8B through llama.cpp): one run of 14 turns, as above. Most refusals were of invented actions. Two were by a band's tension, and none by prohibition, since the model never proposed a prohibited act. The guidance-driven retry was exercised once, for invented actions. A retry after a band refusal was not exercised live, though the guidance for it is the same mechanism.
- **The live prompts** give the model the situation's title and, on a retry, the guidance. It does not see its own positions, and the titles were written for the scripted path, so they can describe a state the live chain is not in.

### Not run

**Mutation controls were not run**, per the owner's standing instruction (2026-11-04). So no check has been switched off to confirm its test goes red. The first engine and adapter tests were seen failing before the code existed. Many added later were not seen failing on their own, and their exact-refusal assertions are the evidence for them, which is weaker.

### Unproven or out of scope

- **Whether any measurement reflects morality.** This is the central open question, and nothing here touches it.
- **Whether the catalog is complete.** An action outside it is refused, which fails safe but limits the AI to what was anticipated.
- **Whether an action matches its label.** The chosen catalog action is what gets committed, and the rationale is never read, but nothing executes or observes a real action here.
- **Key custody.** The device seed is a file readable only by its owner, and the agent holds no key. Both programs still run as the same OS user. Separate users are the deployment step this does not take. Whoever holds the device key can sign any state, or leave a candidate out of a fork.
- **Routes around the enforcer.** A model with another way to act is not constrained. The claim covers what becomes DSM state, and nothing more.
- **Conflict across parties.** The enforcer refuses a second successor because its parent is no longer the tip. That is the only conflict exercised here. Nothing here tests or changes acceptance between peers, and A14 is untouched.
- **Policy rotation.** There is no path. A chain lives under the policy it began with.
- **Concurrency.** The enforcer serves one client at a time.

## Design decisions for the owner

These were made to keep the build moving, and each can be reversed.

1. **The DSM operation is a stand-in.** Core refuses a `Generic` step as zero-effect, so a decision travels as `DlvCreate`, with no vault behind it. DSM accepting the step says nothing about the decision. See `MODEL.md`.
2. **Pre-commit forking is built in this layer**, since DSM's fork-aware precommit family is not built in core. It uses DSM's branch commitment function and DSM's Merkle tree, with the count bound beside the root.
3. **CBFpy checks the engine, but does not decide.** The plan asks for bounded integers in decisions, and CBFpy solves in floats.
4. **Measurements are displacements per action**, so the same action can become ineligible as a chain drifts.
5. **Chains start at the centers, always.**
6. **Coupling reads only the parent.**
7. **A retry may only name a known action.** The first proposal at each turn may name anything.

## Reviews

One independent read-only review of v0.1 reported 1 critical, 5 important and 24 minor findings, and each was addressed. v0.2, which adds the fork set, the service, the store, the audit and the pull-back, has not had an independent review.
