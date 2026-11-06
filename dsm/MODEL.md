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

`lean/BandInvariant.lean` proves these on a model of the rule, and the restoration bound for a fixed threshold. It is not linked to the Rust code. See `REPORT.md` for exactly what that covers.

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
| Decision commitment | Parent tip, parent state, successor, policy hash and version, branch, evaluation, evidence |
| DSM step | The decision, carried in an operation, moved through `prepare_advance_relationship` and `commit_advance` |
| Attested state | Sequence, new tip, positions, policy, catalog and the entropy core's next transition reads, under the device key |

A verifier takes one parent state and one step. It reruns the engine, recomputes every commitment, derives the step's entropy from the parent with core's rule, and recomputes the tip with DSM's `relationship_chain_tip_v2`. A genesis parent must equal the centers and core's own `genesis_head` for that device. Its signature gives it no means of reading anything earlier.

### The operation is a stand-in

Core refuses `Operation::Generic` as a zero-effect step (MR-DSM-0201), so a decision is carried as `Operation::DlvCreate`, which core admits and which moves no value. **No vault exists.** The fields hold other things than their names say: `vault_id` is the decision commitment, `parameters_hash` the policy hash, and `fulfillment_condition` the successor commitment. Core does not check this variant's signature, so DSM accepting a step says nothing about the decision. Only this layer's verifier reads it. Which operation should carry a decision is an open choice for the owner, since DSM itself is not changed here.

### Pre-commit forking

`dsm/src/commitments/precommit.rs` states that DSM's fork-aware precommit family, with several candidates under one committed root, is not built. This prototype builds its forking here in the policy layer. Each candidate gets its own branch commitment and nothing commits a set of candidates to one root. Tripwire's rule that only one branch of a parent can be realized is relied on only as far as this enforcer's own head: it refuses any future whose parent is no longer the tip.
