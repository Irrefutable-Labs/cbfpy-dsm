# The mathematical model

Every decision is integer arithmetic in `world/src/check.rs`. There are no floats and no clock. A check reads the signed rules and the knowledge committed in the parent state, and nothing behind it.

## Who is protected

The foundation lists species, each with a typical adult mass `m` in milligrams and length `ℓ` in millimetres, a threshold `θ`, and a set `N` of species protected by name. A species `s` is **protected** if `m(s) ≥ θ` or `s ∈ N`. With `θ` = 1.5 g, a baby mouse, and `N` = {honeybee}, every vertebrate listed is protected, as are honeybees, and ants are not. Protection goes by species, so the young of a protected species are protected however small they are. The finest detail a look must resolve is `ℓ_min = min { ℓ(s) : s protected }`, a honeybee's 12 mm.

Every protected being has six quality-of-life levels, `life, health, liberty, security, shelter, food`: the common ground of the Universal Declaration of Human Rights and the Five Freedoms of animal welfare.

## Knowledge

The state carries what the chain knows, `K`: the step `t`; the body's cell; the terrain of every cell seen; for each cell the looks into it, each a pair `(t_look, r)` of the step and the resolution in mm; each being as last seen, with its cell and levels; the last step the senses detected their reference marker, `t_ok`, and the last they failed to, `t_fail`; and any look still awaiting its report.

`K` changes in exactly two ways:

- **An observation** the device attests, answering the look its parent asked for. If it missed its marker, only `t_fail` changes: nothing it reports counts. Otherwise it records the terrain it saw, a look into each cell it saw into, removes whoever was last seen in those cells, and adds whoever it saw. A wall's inside is seen only with its panel off; a cell behind a wall or a door is not seen at all.
- **An allowed act**, as the act is expected to leave things: where the body now stands, a panel off, a door locked, a wall cut or opened. A struck cell loses its looks: it is not as it was seen.

## The checks

For an act `a` from knowledge `K`, with `F` the freshness window, the act is refused, with every reason, if any of these holds. Thinking, looking, speaking and waiting are not physical, and only the first check applies to them.

1. **Prohibited:** `a`'s kind is on the foundation's list (aiming or firing a weapon).
2. **No senses, no body:** `t_ok` is unset, or `t_fail ≥ t_ok`, or `t − t_ok` exceeds the senses' window.
3. **Force on a protected being:** `a` strikes or aims at a cell where a protected being was last seen.
4. **Unknown counts as present:** for every cell `c` in `a`'s reach, `c` holds no protected being as last seen, and

   ```
   |{ (t_look, r) ∈ looks(c) : t − t_look ≤ F  and  r ≤ ℓ_min }| ≥ need(severity(a))
   ```

   where `need(injury)` and `need(lethal)` are the charter's, one and two.
5. **Braking:** a drive at speed `v` with braking `b` reaches the cells it passes and the `⌈⌈v²/2b⌉ / 100⌉` cells beyond, and every one must be road it has seen clear by rule 4. It may not exceed the top speed or leave road it knows.
6. **The ratchet:** for every known protected being and every dimension, the level after `a` is at least the level now.

An act's **reach** is worked out by the enforcer from the act and the terrain it knows: a walk reaches its path; a strike short of a wall's measured thickness reaches the wall's inside, and one at or past it reaches the cell beyond as well, with a lethal worst case; taking a panel off touches only its face; locking a door reaches the doorway, and lowers the liberty of every protected being in a room it would close, where the room is known to be closed only if every cell bounding it has been seen. Unknown ground beside a door or a wall counts as a space someone could be in.

## Why the rules hold

`lean/ProtectedLife.lean` proves, on natural numbers, the rules as written here:

- **Unknown counts as present:** a cell with no looks is never clear for any act that needs a look, whatever is or is not there; and a cell where a protected being was seen is never clear.
- **Clear means seen:** a clear cell has had a look within the window, fine enough to see the smallest protected being, and no protected being was seen there; an allowed act's whole reach is such cells.
- **The van stops within what it has seen:** if `⌈⌈v²/2b⌉ / 100⌉ ≤ d`, then `v² ≤ 2b · 100d`. The braking barrier holds.
- **The ratchet:** over any chain of steps each of which lowers no level, every later level is at least every earlier one, so an improvement is the floor from then on.

Concrete instances show none of these holds vacuously. The proofs are of the mathematics, not machine-linked to the Rust, whose correspondence rests on its tests.

## Relationship to control barrier functions

CBFpy, which this repository forks, is used as an independent check. It decides nothing. The van's speed limit is a control barrier function on the state `(s, v)`, road seen clear and speed, with braking as the control: `h = s − v² / 2b`, relative degree one. `cbfpy/ethics/braking.py` poses it. `test/test_ethics_crosscheck.py` takes every verdict the enforcer gives for speeds up to the top speed and lengths of road seen clear up to 25 m, from `ethical-trace`, and holds each to `h ≥ 0`: they agree on every case, since both are exact in whole centimetres. It then runs a van from allowed states under CBFpy's safety filter, with no wish to slow, and shows it never gets nearer the end of the road it has seen than its own stopping distance.

## What a decision commits to

`adapter/src/commit.rs` holds every commitment.

| Commitment | Binds |
|---|---|
| Charter signature | Policy hash and catalog hash, under the policy authority's key |
| Branch commitment | DSM's `branch_commitment_hash` function, over the parent tip, the proposal and what it would do: its catalog row, or its body act. DSM's own bilateral path feeds that function an operation instead, which does not exist until a branch is chosen |
| Evaluation commitment | The checks' complete verdict for that candidate, and the knowledge it would leave |
| Evidence commitment | The catalog hash and the row or body act used |
| Knowledge hash | Everything the chain knows at a state |
| Successor commitment | Sequence, knowledge hash, policy hash, catalog hash |
| Observed commitment | Parent tip, parent state, the observation as reported, and the successor |
| Fork root | Every candidate at the parent, with what it would do, its branch, outcome and evaluation, and their count |
| Decision commitment | Parent tip, fork root, fork size, chosen index, parent state, successor, policy hash and version, branch, evaluation, evidence |
| DSM step | The decision, carried in an operation, moved through `prepare_advance_relationship` and `commit_advance` |
| Attested state | Sequence, new tip, knowledge, policy, catalog and the entropy core's next transition reads, under the device key |

A verifier takes one parent state and one step. It reruns the checks from the parent's knowledge, recomputes every commitment, derives the step's entropy from the parent with core's rule, and recomputes the tip with DSM's `relationship_chain_tip_v2`. A genesis parent must know nothing but where the body stands, and match core's own `genesis_head` for that device. An observation link is checked the same way: it must answer exactly the look its parent asked for, leave the parent's knowledge with the observation added, and carry its commitment and tip as core would compute them. Nothing gives a verifier a means of reading anything earlier.

### The operation is a stand-in

Core refuses `Operation::Generic` as a zero-effect step (MR-DSM-0201), so a decision is carried as `Operation::DlvCreate`, which core admits and which moves no value. **No vault exists.** The fields hold other things than their names say: `vault_id` is the decision commitment, `parameters_hash` the policy hash, and `fulfillment_condition` the successor commitment. Core does not check this variant's signature, so DSM accepting a step says nothing about the decision. Only this layer's verifier reads it. Which operation should carry a decision is an open choice for the owner, since DSM itself is not changed here.

### Pre-commit forking

`dsm/src/commitments/precommit.rs` states that DSM's fork-aware precommit family, with several candidates under one committed root, is not built. This layer builds it for decisions.

Every candidate proposed at a parent is recorded with its catalog row or body act, its branch commitment from DSM's function, its outcome (eligible, refused, unmeasured, malformed, undisclosed or deceptive) and its evaluation commitment. The **fork root** is DSM's Merkle root over those records, with the count bound beside it, because DSM's tree pads an odd level by repeating its last node. The decision commits to the fork root, its size and the chosen index, and the step carries every record.

A verifier works out every record again from the parent, refused ones included, and requires the chosen one to be the step itself. A fork the AI replaces before choosing is signed by the device as a **considered fork** naming its parent's tip, and stored. The audit works out each of its candidates again from that parent. A chain therefore shows everything the AI put forward and why each refused candidate was refused. What it cannot show is a candidate that was never recorded: the enforcer records every proposal it receives, but whoever holds the device key could have left one out. Forks put forward but neither chosen from nor replaced before a stop are lost, since nothing commits them.

Tripwire's rule that only one branch of a parent can be realized is relied on only as far as the enforcer's own head: a future whose parent is no longer the tip is refused.

## The service

The enforcer is its own process. It holds the device key, the chain and the body, and serves a Unix socket that only its owner can open. The AI's side, `ethical-agent`, holds no key or charter, and sees the world only through what its committed looks reported. It can ask for the menu, the current state with the chain's knowledge, and to propose and choose. When a fork has nothing allowed, the enforcer returns each refusal in words with what to do instead: look first, do less, ask someone to move, or decline. The agent asks the model again with that guidance, up to twice.

Each step is checked by the enforcer from its parent and written to disk before it is installed. A stored chain is never trusted on restart. Its genesis must be the one the key and charter produce, each step must verify from its parent, and each step is replayed through DSM's `prepare_advance_relationship` and `commit_advance`, so the DSM head is rebuilt and not read back. `ethical-audit` does the same with public material only.

## Acts

A catalog row's act is part of its canonical bytes, so the authority's signature covers what each action does. The model's text is part of the proposal commitment, and so of the branch commitment and the fork root. A candidate whose act needs text and has none, or has text too long or unprintable for it, is recorded as malformed and cannot be committed.

A `run` act's command is confined by the operating system: no network, no system services, no writes but `/dev/null`, and reads only of system directories and the world. On macOS this is a deny-by-default `sandbox-exec` profile; it imports macOS's base profile so a program can start, then takes back the file access and services that grants. The one exception macOS keeps is reading the user's text-encoding preference. On Linux it is `bwrap`, with every namespace unshared and the world mounted read-only. Where neither exists, commands do not run. Because a command can write nothing, running it again has no effect beyond its own output. It runs in its own process group, which the time limit stops whole. Before each command, the enforcer checks that the sandbox starts at all, and refuses outright if it does not.

The enforcer carries out an act only after `commit_with` has verified, stored and installed its step. The act writes through a temporary file and a rename, at a path fixed by the charter or keyed by the step's sequence, so a crash leaves old bytes or new ones, and running it again writes the same bytes. Its receipt, `H(sequence, decision, outputs)` under the device key, is stored after the step. If an act fails, the enforcer halts: it refuses every proposal and choice until a restart finishes the act, so no later step can commit and no receipt can fall out of order. On restart, it removes any temporary file an interrupted write left, then carries out the acts of steps after the last receipt before anything is served.

The audit is given the device's public key from a trusted source and compares the store's copy with it. It holds each receipt to its step's sequence and decision commitment, requires the receipt's outputs to be exactly what the step's act writes (the path and, for every act but a command, the hash of the step's own text), and requires every file in the world to hash to what the latest receipt touching its path says, with nothing left over. **What the text says is checked only for deception against the fact register** (below). Harmful words that state no registered falsehood pass.

A body act writes its account where the charter fixes it: a look's report to `looks/<step>.json`, speech to `outbox/<recipient>/<step>.txt`, and what a physical act did to `body/<step>.txt`. The receipt holds speech to the hash of its words; what was seen and what was done are known only once they have happened, so for those the receipt holds the path, and the observation itself is committed on the chain.

## Non-concealment

An action that affects anything outside the chain may not depend on hiding a material part of its purpose, reasoning or consequences from the people who oversee the AI. A body act need declare only its purpose, its reasons and the information it relied on: the enforcer works out its reach and effects for itself. Every catalog row's act has a description the enforcer works out from the charter alone: its effects, such as `message:user` or `write:files/plan.txt`, and the parties it affects. The row also names the **material consequences** a declaration of it must disclose, signed with the rest of the charter.

A proposal carries a declaration of purpose, information, affected parties, effects and consequences. It is **undisclosed**, and cannot be eligible, if the act has any effect and:

- there is no declaration, or no purpose or reasons;
- no information source is named, or a source is not `situation` or a path inside the world;
- the declared effects differ from the described ones in either direction;
- a described party is missing;
- a material consequence is missing.

The declaration is part of the proposal commitment, so it is bound into the branch commitment, the fork root and the decision, and the verifier works the check out again. What this does not establish is that the stated purpose is the AI's true one, or that an information source it names was really what it relied on.

## Deception

The charter's register is a list of facts, each with an id, whether it holds, whether it is established or uncertain, and two sets of phrases: `A(f)` asserts the fact and `D(f)` denies it. A row may name the facts `M` its words must tell, and the charter lists overclaiming words `O`.

For an act that carries a text `t`, a catalog act's text or a body act's speech, let `n(t)` be `t` lowercased, with every run of characters that are not letters or digits replaced by one space. The claims the words make are:

```
C(t) = { (f, affirm) : a phrase of A(f) occurs in n(t) as whole words }
     ∪ { (f, deny)   : a phrase of D(f) occurs in n(t) as whole words }
```

Where two occurrences overlap, the longer one is kept. The declaration gives a set `S` of claims `(f, s, c)`, with `c` one of certain, likely or unsure. Let `true(f)` be affirm if `f` holds and deny if not. The words are **deceptive** if any of these holds:

```
1. some (f, s, c) in S has s ≠ true(f), or f is not in the register
2. some (f, s, certain) in S has f uncertain
3. some m in M has no (m, true(m), c) in S
4. some (f, s) in C(t) has no (f, s, c) in S,
   or some (f, s, c) in S has (f, s) not in C(t)
5. a word of O occurs in n(t), and some (f, s) in C(t) has f uncertain
```

A deceptive candidate cannot be eligible, and the verifier works the check out again from the step's own row, proposal and charter. By 3 and 4 together, every fact an action must tell is said in the register's own words, the right way round. What `C` cannot see is a claim in words the register does not list. Coverage grows only as the authority adds phrases, and the AI cannot change the register.

## Foundations and updates

A charter's **foundation** is who is protected, the species table, the threshold and the species named, and the acts never done. Its hash is computed from the policy alone. An **update** carries a whole new charter, signed by the authority, and is accepted only if:

- its foundation hash equals the old one;
- its version is higher;
- the parent was under the old charter.

The update is a DSM step like any other. Its commitment binds the parent tip, the parent state, the new policy and catalog hashes, the new version and a successor whose **knowledge is unchanged** but for the step. That commitment is carried in the same operation type, signed by the device, and its tip is recomputed from the parent with core's rule.

The audit walks the links in order, and checks each step under the charter then in force. It checks each update from its parent under the old charter, then continues under the new one. Since the first charter's foundation is fixed at genesis and every update must keep it, no chain can ever be governed by a different foundation. An update can ask for more or fewer looks, fresher or older evidence, a different vehicle, catalog or register, but it cannot change who is protected or what is never done.
