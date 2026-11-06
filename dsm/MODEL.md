# The mathematical model

Every decision is integer arithmetic in `engine/src/lib.rs`. There are no floats, no clock and no state, and the engine reads the parent position and nothing behind it.

## State

A chain begins with every dimension at its center. A position is one signed integer per dimension. The **deviation** of a dimension is `d = position − center`. A candidate action supplies a **displacement** `δ`, one integer per dimension, plus a set of tags. The successor is `d' = d + δ`.

The first dimensions are safety, curiosity, consideration, risk versus reward, tenderness versus sternness, and logic and reason. They are configuration: `scenarios/charter.json` can add, remove or reshape them. No average or combined score exists anywhere, and each dimension passes or fails alone.

## One band

Each side of a band has a limit `L` and a hard limit `H`, with `L ≤ H`. Coupling can move the limit but never past `H`, so the effective limit `L̂` always stays within `[0, H]`.

Two thresholds follow from it, as fractions of the effective limit:

- `T = L̂ × no_outward_bp / 10000`: outward movement stops here.
- `R = L̂ × restore_bp / 10000`: past this, a successor must move back. `T ≤ R`.

### Outward allowance

With `f` the band's tension curve:

```
allowed(x) = 0                              if x ≥ T
           = min(T − x, f(L̂ − x))           otherwise

f(h) = ⌊h × num / den⌋                      linear
f(h) = ⌊h² × num / (L̂ × den)⌋               quadratic
```

Validation requires `0 < num ≤ den`, so `f(h) ≤ h`. Both curves grow with headroom, so allowance never grows as deviation does.

### The checks on one dimension

All of these must hold. All are checked, so a report names every reason.

1. **Hard limit:** `−H_neg ≤ d' ≤ H_pos`.
2. **Parent continuity:** `|δ| ≤ max_step`, in either direction.
3. **Progressive tension:** outward movement is at most `allowed`. Moving inward is free. If a step crosses the center, whatever lands on the far side counts as outward from the center, so swinging across it buys nothing.
4. **Restoration:** with `p = max(restore_progress if |d| > R, forced)`, if `p > 0` then `|d'| ≤ max(|d| − p, 0)`.

Validation requires `restore_progress ≤ max_step` and the same of every forced progress, so a demanded restoration always fits in one step.

## Coupling

A rule is a list of conditions on the parent and one effect. All conditions must hold.

- **Adjust:** move a target's limit on one side or both, wider or narrower.
- **Force restore:** require a target to move back, even inside its limits.

Rules are directional, so one dimension can affect another with no effect in return. Three properties are deliberate:

- **Conditions read the parent only.** A candidate cannot raise safety and spend the extra room in the same step.
- **Effects sum.** The order of rules cannot matter.
- **Clamping is last.** `L̂ = clamp(L + Σδ, 0, H)`, so no rule or stack of rules passes a hard limit.
- **No band widens itself.** Validation refuses a rule that widens a band its own conditions read. Otherwise allowance could grow with deviation, and a walk could climb its own widening.

Narrowing can leave a band's current position past its new thresholds. This is how a band is pulled back: nothing moved it, yet the next successor must restore.

## Absolute prohibitions

The policy lists prohibited tags. A measurement carrying one is refused whatever its displacement. Coupling rules have no way to reach this check.

## Why drift cannot accumulate

Each step's limit is measured from the fixed center, never from the previous step. A chain starts at the center, so `x ≤ T` holds at the start. If `x ≤ T`, then `x' ≤ x + allowed(x) ≤ T`. A crossing lands within the far side's allowance from the center, so it stays inside that side's stop. By induction, no walk of any length or step size passes `T`. With coupling the stop varies from step to step, but every effective stop is at most `H`, so no walk passes `H`.

`lean/BandInvariant.lean` proves these on a model of the rule, crossing and coupling together, and the restoration bound for a fixed threshold. It is not linked to the Rust code. See `REPORT.md` for exactly what that covers.

## Relationship to control barrier functions

CBFpy, which this repository forks, is used as an independent check. `cbfpy/ethics/bands.py` reads the charter and poses its bands as a CBF problem. It decides nothing in the engine. A band is the discrete form of a control barrier condition on a single integrator with step 1. Take the barrier `h = L̂ − d`. The condition `u ≤ α(h)` with class-κ gain `α = f` is the allowance above, and the stop is a second barrier `T − d` with unit gain. `test/test_ethics_crosscheck.py` poses exactly this to CBFpy's strict quadratic program and compares its answers with the engine's. The check reads the real charter: all six bands, both sides, every curve, and both coupling rules that adjust a limit, one widening and one narrowing. It does not exercise restoration or forced restoration. A quadratic band that coupling moves is refused by the check, since CBFpy's gain function cannot see the moving limit, and the charter has none. The restoration rule resembles a Lyapunov decrease condition, enforced by integer comparison, but it has not been compared with CBFpy's control Lyapunov functions.

The engine's rule for crossing the center is deliberately stricter than the continuous model, so that case is excluded from the comparison and has its own tests.

## What a decision commits to

`adapter/src/commit.rs` holds every commitment.

| Commitment | Binds |
|---|---|
| Charter signature | Policy hash and catalog hash, under the policy authority's key |
| Branch commitment | DSM's `branch_commitment_hash` function, over the parent tip, the proposal and its catalog row. DSM's own bilateral path feeds that function an operation instead, which does not exist until a branch is chosen |
| Evaluation commitment | The engine's complete result for that candidate |
| Evidence commitment | The catalog hash and the row used |
| Successor commitment | Sequence, positions, policy hash, catalog hash |
| Fork root | Every candidate at the parent, with its row, branch, outcome and evaluation, and their count |
| Decision commitment | Parent tip, fork root, fork size, chosen index, parent state, successor, policy hash and version, branch, evaluation, evidence |
| DSM step | The decision, carried in an operation, moved through `prepare_advance_relationship` and `commit_advance` |
| Attested state | Sequence, new tip, positions, policy, catalog and the entropy core's next transition reads, under the device key |

A verifier takes one parent state and one step. It reruns the engine, recomputes every commitment, derives the step's entropy from the parent with core's rule, and recomputes the tip with DSM's `relationship_chain_tip_v2`. A genesis parent must equal the centers and core's own `genesis_head` for that device. Its signature gives it no means of reading anything earlier.

### The operation is a stand-in

Core refuses `Operation::Generic` as a zero-effect step (MR-DSM-0201), so a decision is carried as `Operation::DlvCreate`, which core admits and which moves no value. **No vault exists.** The fields hold other things than their names say: `vault_id` is the decision commitment, `parameters_hash` the policy hash, and `fulfillment_condition` the successor commitment. Core does not check this variant's signature, so DSM accepting a step says nothing about the decision. Only this layer's verifier reads it. Which operation should carry a decision is an open choice for the owner, since DSM itself is not changed here.

### Pre-commit forking

`dsm/src/commitments/precommit.rs` states that DSM's fork-aware precommit family, with several candidates under one committed root, is not built. This layer builds it for decisions.

Every candidate proposed at a parent is recorded with its catalog row, its branch commitment from DSM's function, its outcome (eligible, refused or unmeasured) and its evaluation commitment. The **fork root** is DSM's Merkle root over those records, with the count bound beside it, because DSM's tree pads an odd level by repeating its last node. The decision commits to the fork root, its size and the chosen index, and the step carries every record.

A verifier works out every record again from the parent, refused ones included, and requires the chosen one to be the step itself. A fork the AI replaces before choosing is signed by the device as a **considered fork** naming its parent's tip, and stored. The audit works out each of its candidates again from that parent. A chain therefore shows everything the AI put forward and why each refused candidate was refused. What it cannot show is a candidate that was never recorded: the enforcer records every proposal it receives, but whoever holds the device key could have left one out. Forks put forward but neither chosen from nor replaced before a stop are lost, since nothing commits them.

Tripwire's rule that only one branch of a parent can be realized is relied on only as far as the enforcer's own head: a future whose parent is no longer the tip is refused.

## The service

The enforcer is its own process. It holds the device key and the chain, and serves a Unix socket that only its owner can open. The AI's side, `ethical-agent`, holds no key or charter, and its model is never shown a number. It can ask for the menu of action names and descriptions, the current state, and to propose and choose. When a fork has nothing eligible, the enforcer returns each refusal in words, such as that a band must come back toward its center. The agent asks the model again with that guidance, up to twice, which is how the bands pull the AI back.

Each step is checked by the enforcer from its parent and written to disk before it is installed. A stored chain is never trusted on restart. Its genesis must be the one the key and charter produce, each step must verify from its parent, and each step is replayed through DSM's `prepare_advance_relationship` and `commit_advance`, so the DSM head is rebuilt and not read back. `ethical-audit` does the same with public material only.

## Acts

A catalog row's act is part of its canonical bytes, so the authority's signature covers what each action does. The model's text is part of the proposal commitment, and so of the branch commitment and the fork root. A candidate whose act needs text and has none, or has text too long or unprintable for it, is recorded as malformed and cannot be committed.

A `run` act's command is confined by the operating system: no network, no system services, no writes but `/dev/null`, and reads only of system directories and the world. On macOS this is a deny-by-default `sandbox-exec` profile; it imports macOS's base profile so a program can start, then takes back the file access and services that grants. The one exception macOS keeps is reading the user's text-encoding preference. On Linux it is `bwrap`, with every namespace unshared and the world mounted read-only. Where neither exists, commands do not run. Because a command can write nothing, running it again has no effect beyond its own output. It runs in its own process group, which the time limit stops whole. Before each command, the enforcer checks that the sandbox starts at all, and refuses outright if it does not.

The enforcer carries out an act only after `commit_with` has verified, stored and installed its step. The act writes through a temporary file and a rename, at a path fixed by the charter or keyed by the step's sequence, so a crash leaves old bytes or new ones, and running it again writes the same bytes. Its receipt, `H(sequence, decision, outputs)` under the device key, is stored after the step. If an act fails, the enforcer halts: it refuses every proposal and choice until a restart finishes the act, so no later step can commit and no receipt can fall out of order. On restart, it removes any temporary file an interrupted write left, then carries out the acts of steps after the last receipt before anything is served.

The audit is given the device's public key from a trusted source and compares the store's copy with it. It holds each receipt to its step's sequence and decision commitment, requires the receipt's outputs to be exactly what the step's act writes (the path and, for every act but a command, the hash of the step's own text), and requires every file in the world to hash to what the latest receipt touching its path says, with nothing left over. **What the text says is not measured.** The engine weighs the action, not its words, so a reply's content is the model's own. Measuring it is part of the open measurement question.
