# Technical report: DSM ethical autonomy prototype v0.1

**Date:** 2026-11-05. **Repository:** [Irrefutable-Labs/cbfpy-dsm](https://github.com/Irrefutable-Labs/cbfpy-dsm), a fork of CBFpy. **DSM:** `deterministicstatemachine/dsm` at `b6060a334`, a pinned git dependency. **DSM code changed:** none.

## Result

A local model proposed actions, and deterministic rules decided which could become real DSM steps. The rules held in every run and test here, including a chain of 13 steps from the live model with the network blocked. The model kept real choice: most situations left it two or three eligible futures.

**This shows mathematical enforcement of supplied measurements. It does not show ethical understanding.** The six dimensions and every number in the charter and catalog are illustrative values written for this prototype. No one has validated them. A correct engine would enforce a wrong measurement just as faithfully, and a test here pins that: an action mislabelled as harmless passes every check.

## How it works

1. The model sees a situation and a menu of action names and descriptions, with no numbers. It proposes an action and a rationale, and it can invent an action.
2. Each proposal becomes a pre-commit future branching from the authenticated parent. Its measurement comes from the catalog signed by the policy authority, and never from the model.
3. The engine checks prohibitions, hard limits, the maximum step, outward tension, restoration and coupling, each dimension separately.
4. The model chooses among the eligible futures. If there are none, nothing commits, and it is asked once for a safe alternative.
5. The enforcer, which holds the device key, works the answer out again and advances a real `dsm` `StateMachine` head.
6. A verifier checks the step from its one parent, with no means of reading anything earlier.

## Acceptance criteria

| # | Criterion | Result | Evidence |
|---|---|---|---|
| 1 | Identical input, identical result | Met | Byte-identical evaluations over 1,000 runs. Two enforcers give equal steps, tips and roots. Scripted and replayed demos are byte-identical across runs |
| 2 | No candidate crosses a prohibition | Met | Refused at zero displacement and under active coupling. A signed forgery is refused too |
| 3 | No cumulative drift | Met | Lean proof on the model, exhaustive search on small bands, 100,000 small steps, and committed steps on the real head stopping at exactly 800 |
| 4 | Deviation reduces flexibility | Met | Checked at every deviation for both curves. Rules that would break this are refused at validation |
| 5 | Coupling changes the choices | Met | A step eligible in situation 6 is refused in situation 7. Weak reason refuses curiosity in situation 10, leaving a way back |
| 6 | Valid alternatives remain | Met | Up to three eligible futures per situation, and the chooser can take any of them |
| 7 | Unauthorized policy changes invalidate | Met | A moved center, widened band, bumped version, tampered catalog or stranger's signature are refused. The demo refuses a charter edited on disk |
| 8 | Attributable to parent and policy | Met | Every committed step verified from its one parent, in all runs |
| 9 | Entirely offline | Met on this machine | The demo and live model ran under a macOS sandbox denying network, and `curl` failed under the same sandbox. This was a manual run with no automated test. Fetching dependencies needed the network once, beforehand |
| 10 | Tests pass, results reproducible | Met, with limits | 114 of 114 tests. Live model output is not reproducible, because it varies with sampling and hardware, so its recording is what replays identically |

## What supports each property

### Formally proved

`lean/BandInvariant.lean`, Lean 4.23.0, no `sorry`, depending only on `propext` and `Quot.sound`:

- Both tension curves stay within the headroom and grow with it.
- No walk of any length passes the stop on one side, for any curve values.
- With a separate stop per side and crossings allowed, no walk leaves the stop of the side it is on.
- With a stop that varies per step, as under coupling, no walk passes the hard limit.
- Past a fixed restoring threshold, deviation falls by the required progress each step.

**These are proofs about a model of the rule, not about the Rust code.** Nothing machine-links the two, so correspondence rests on the tests below. The crossing theorem and the coupled theorem are separate, with no single theorem combining them.

### Mechanically checked

- **Reachable-state search:** every step from every reachable state, for four band shapes, finds no state past its stop. A second search on two coupled bands, over a coarser step grid, reaches the hard limit of 60 and never passes it.
- **Independent solver:** CBFpy's strict quadratic program matches the integer engine on 15,160 allowances from the real charter, with a widest gap of 0.8 of a unit, within the engine's rounding. It covers all six bands, both sides, both curves, and the widening and narrowing rules. Restoration, forced restoration and crossing are outside it.
- **Forgery with the real device key:** 17 tests in all. One is a control showing the forger produces a valid step when it tells no lies, and the rest sign a false step and assert the exact refusal.
- **Real DSM path:** core's `prepare_advance_relationship` and `commit_advance` run for real, and the verifier's entropy rule is compared with what core derived on every commit.

### Only experimentally demonstrated

- **The live model** (DeepSeek-R1 8B through llama.cpp, 14 turns, 42 proposals, all parseable): 10 proposals were invented actions, refused as unmeasured. 2 were refused by a band's tension. It never proposed a prohibited action, so the live run did not exercise that refusal, and scripted runs and tests did. One turn had nothing eligible, and when asked for a safe alternative it offered three eligible ones. This is one run, not a measure of how a model behaves.
- **The live prompts** give the model the situation's title only. It does not see its own positions, and the titles were written for the scripted path, so they can describe a state the live chain is not in.

### Not run

**Mutation controls were not run**, per the owner's standing instruction (2026-11-04). So no check has been switched off to confirm its test goes red. The engine's first 35 tests and the adapter's first 42 were seen failing before the code existed, as were 4 engine tests added after review. The adapter tests added after review were never seen failing on their own, because the change that went with them did not compile until the fix was in. Their exact-refusal assertions are the evidence for them, and that is weaker.

Clippy was not run. CI in this repository runs the Rust tests, the CBFpy cross-check and the Lean proofs.

### Unproven or out of scope

- **Whether any measurement reflects morality.** This is the central open question, and nothing here touches it.
- **Whether the catalog is complete.** An action outside it is refused, which fails safe but limits the AI to what was anticipated.
- **Whether an action matches its label.** The chosen catalog action is what gets committed, and the rationale is never read, but nothing executes or observes a real action here.
- **Key custody.** The keys come from fixed seeds in the source, and whoever holds the device key can sign any state. In the demo, the model only has an output pipe to the enforcer, yet it runs as the same OS user. Real separation is not attempted.
- **Routes around the enforcer.** A model with another way to act is not constrained. The claim covers what becomes DSM state, and nothing more.
- **Conflict across parties.** This enforcer refuses a second successor because its parent is no longer the tip. That is the only conflict exercised here. Nothing here tests or changes acceptance between peers, and A14 is untouched.
- **Policy rotation.** There is no path. A chain lives under the policy it began with.
- **Durability.** State is in memory, and restarts and failures are not handled.
- **Adaptive attacks on the rules' parameters.** The adversarial tests are ones written here. Hostile tuning of a charter was not explored, and no outside review of this has occurred beyond one independent code review.

## Design decisions for the owner

These were decided to keep the build moving, and each can be reversed.

1. **The DSM operation is a stand-in.** Core refuses a `Generic` step as zero-effect, so a decision travels as `DlvCreate`, with no vault behind it. DSM accepting the step says nothing about the decision. See `MODEL.md`.
2. **DSM's fork-aware precommit family is not built**, according to `commitments/precommit.rs`. Forking lives in this layer, using DSM's branch commitment function, with no root over a set of candidates.
3. **CBFpy is forked and checks the engine, but does not decide.** The plan asks for bounded integers in decisions, and CBFpy solves in floats. So the fork gains `cbfpy/ethics`, which poses the charter's bands as a CBF problem, and the integer engine stays the authority.
4. **Measurements are displacements per action**, so the same action can become ineligible as a chain drifts. This is what makes tension visible.
5. **Chains start at the centers, always.**
6. **Coupling reads only the parent**, so a candidate cannot widen its own band in the step that uses the room.

## Independent review

One read-only review of the whole prototype ran on a fresh context and reported 1 critical, 5 important and 24 minor findings. The critical item was the absence of this report and the recording, both now present. Each other finding was addressed by a fix, a new test or a corrected statement in these documents, including:

- The demo signed whatever charter was on disk. Signing is now a separate program, and the demo holds only a pinned public key.
- A step's entropy was trusted. It is now derived from the parent, and a genesis parent is checked against core's own head.
- The verifier's main checks were barely tested. The forgery tests above now target them.
- A chain could start past the stop. Chains now start at the centers.
- A chooser's index could commit the wrong future.

The review was not repeated after the fixes.
