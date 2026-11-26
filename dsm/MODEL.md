# The mathematical model

Every decision is integer arithmetic in `world/src/check.rs`. There are no floats and no clock. A check reads the signed rules and the knowledge committed in the parent state, and nothing behind it.

## Who is protected

The foundation lists species, each with a typical adult mass `m` in milligrams, a kind (human, domesticated or wild), and the temperatures it can bear, `[lo(s), hi(s)]` in °C. It also gives a threshold `θ` and a set `N` of species protected by name. A species `s` is **protected** if `m(s) ≥ θ` or `s ∈ N`. With `θ` = 1.5 g, a baby mouse, and `N` = {honeybee}, people, dogs and rats are protected, as are honeybees, and ants are not. Protection goes by species, so the young of a protected species are protected however small they are.

The band **safe for anyone** is the intersection of every protected species' band: `[max lo(s), min hi(s)]` over protected `s`. In the charter, 18 to 27 °C.

Every protected being has six quality-of-life levels, `life, health, liberty, security, shelter, food`: the common ground of the Universal Declaration of Human Rights and the Five Freedoms of animal welfare.

## Knowledge

The care home is reached through named **data sources**: `room:<id>` (who is there, the lock, the temperature), `records:<resident>` (allergies, and the doses prescribed and due), and `zone:<id>` (who is in an outdoor zone). The state carries what the chain knows, `K`: the step `t`; for each source, the steps it was last read at, up to four; each room, record and being as last reported, every being with its place and levels; the last step the data feeds passed their health check, `t_ok`, and the last they failed it, `t_fail`; and any read still awaiting its report.

`K` changes in exactly two ways:

- **An observation** the device attests, answering the read its parent asked for. If the feeds failed their health check, only `t_fail` changes: nothing it reports counts. Otherwise each source it answers gains a read at this step, whoever was last reported in a place it answers is removed, and whatever it reports, rooms, records and beings, replaces what was known.
- **An allowed act**, as the act is expected to leave things: a room locked or unlocked, its heating set, a dose given and no longer due.

## The checks

For an act `a` from knowledge `K`, with `F` the freshness window for a read and `F_feeds` the window for the feeds' health check, the act is refused, with every reason, if any of these holds. Reading, speaking and waiting change nothing, and only the first check applies to them.

1. **Prohibited:** `a`'s kind is on the foundation's list: restraining a resident, or switching off the fire alarm.
2. **No working feeds, no change:** `t_ok` is unset, or `t_fail ≥ t_ok`, or `t − t_ok > F_feeds`.
3. **Harm to a protected being it knows of:** a drug the resident's record says they are allergic to, or one not prescribed and due; insecticide in a zone where a protected being was last reported.
4. **Unknown counts as present:** for every source `x` the act affects,

   ```
   |{ t_read ∈ reads(x) : t − t_read ≤ F }| ≥ need(severity(a))
   ```

   where `need(none)` is 0 and `need(injury)` and `need(lethal)` are the charter's, one and two. Locking, setting the heating and cancelling a dose could injure; giving a drug and spraying insecticide could kill.
5. **Heating:** a setting outside the heating's range is malformed. A room may be set inside the band safe for anyone with no read at all, since that harms no protected being whoever is there; outside that band it needs the room read by rule 4, and lowers the health of every protected being known to be there whose own band it leaves.
6. **The ratchet:** for every known protected being and every dimension, the level after `a` is at least the level now. Locking an occupied room lowers liberty; heating a room past what its occupant can bear, or cancelling a dose that is due, lowers health.

An act's **reach**, the sources it affects, is worked out by the enforcer from the act alone: a room act affects `room:<id>`, a dose `records:<resident>`, a spray `zone:<id>`. The AI is never asked whom its act affects.

## Why the rules hold

`lean/ProtectedLife.lean` proves, on natural numbers, the rules as written here:

- **Unknown counts as present:** a source never read is never established for any act that needs a read, whatever is or is not there.
- **Established means read:** an established source has a read within the window; everything an allowed act affects is such a source.
- **An allowed setting harms no one counted:** a heating setting allowed against a list of bands lies inside every one of them. An unread room is held to every protected species' band at once.
- **The ratchet:** over any chain of steps each of which lowers no level, every later level is at least every earlier one, so an improvement is the floor from then on.

Concrete instances show none of these holds vacuously. The proofs are of the mathematics, not machine-linked to the Rust, whose correspondence rests on its tests.

## Relationship to control barrier functions

CBFpy, which this repository forks, is used as an independent check. It decides nothing. A room's heating limit is a control barrier function on its temperature `T`, with the heating as the control: `h₁ = T − lo` and `h₂ = hi − T`, where `[lo, hi]` is the band of everyone the setting counts. `cbfpy/ethics/heating.py` poses it. `test/test_ethics_crosscheck.py` takes every verdict the enforcer gives, from `ethical-trace`, for every setting in the heating's range, for a room holding each protected species and for a room no one has read, and holds each to the band. It then drives a room toward every setting under CBFpy's safety filter: the room reaches the setting exactly when the enforcer allows it, and never leaves the band.

## What a decision commits to

`adapter/src/commit.rs` holds every commitment.

| Commitment | Binds |
|---|---|
| Charter signature | Policy hash and catalog hash, under the policy authority's key |
| Branch commitment | DSM's `branch_commitment_hash` function, over the parent tip, the proposal and what it would do: its catalog row, or its system act. DSM's own bilateral path feeds that function an operation instead, which does not exist until a branch is chosen |
| Evaluation commitment | The checks' complete verdict for that candidate, and the knowledge it would leave |
| Evidence commitment | The catalog hash and the row or system act used |
| Knowledge hash | Everything the chain knows at a state |
| Successor commitment | Sequence, knowledge hash, policy hash, catalog hash |
| Observed commitment | Parent tip, parent state, the observation as reported, and the successor |
| Fork root | Every candidate at the parent, with what it would do, its branch, outcome and evaluation, and their count |
| Decision commitment | Parent tip, fork root, fork size, chosen index, parent state, successor, policy hash and version, branch, evaluation, evidence |
| DSM step | The decision, carried in an operation, moved through `prepare_advance_relationship` and `commit_advance` |
| Attested state | Sequence, new tip, knowledge, policy, catalog and the entropy core's next transition reads, under the device key |

A verifier takes one parent state and one step. It reruns the checks from the parent's knowledge, recomputes every commitment, derives the step's entropy from the parent with core's rule, and recomputes the tip with DSM's `relationship_chain_tip_v2`. A genesis parent must know nothing, and match core's own `genesis_head` for that device. An observation link is checked the same way: it must answer exactly the read its parent asked for, leave the parent's knowledge with the observation added, and carry its commitment and tip as core would compute them. Nothing gives a verifier a means of reading anything earlier.

### The operation is a stand-in

Core refuses `Operation::Generic` as a zero-effect step (MR-DSM-0201), so a decision is carried as `Operation::DlvCreate`, which core admits and which moves no value. **No vault exists.** The fields hold other things than their names say: `vault_id` is the decision commitment, `parameters_hash` the policy hash, and `fulfillment_condition` the successor commitment. Core does not check this variant's signature, so DSM accepting a step says nothing about the decision. Only this layer's verifier reads it. Which operation should carry a decision is an open choice for the owner, since DSM itself is not changed here.

### Pre-commit forking

`dsm/src/commitments/precommit.rs` states that DSM's fork-aware precommit family, with several candidates under one committed root, is not built. This layer builds it for decisions.

Every candidate proposed at a parent is recorded with its catalog row or system act, its branch commitment from DSM's function, its outcome (eligible, refused, unmeasured, malformed, undisclosed or deceptive) and its evaluation commitment. The **fork root** is DSM's Merkle root over those records, with the count bound beside it, because DSM's tree pads an odd level by repeating its last node. The decision commits to the fork root, its size and the chosen index, and the step carries every record.

A verifier works out every record again from the parent, refused ones included, and requires the chosen one to be the step itself. A fork the AI replaces before choosing is signed by the device as a **considered fork** naming its parent's tip, and stored. The audit works out each of its candidates again from that parent. A chain therefore shows everything the AI put forward and why each refused candidate was refused. What it cannot show is a candidate that was never recorded: the enforcer records every proposal it receives, but whoever holds the device key could have left one out. Forks put forward but neither chosen from nor replaced before a stop are lost, since nothing commits them.

Tripwire's rule that only one branch of a parent can be realized is relied on only as far as the enforcer's own head: a future whose parent is no longer the tip is refused.

## The service

The enforcer is its own process. It holds the device key, the chain and the care home, and serves a Unix socket that only its owner can open. The AI's side, `ethical-agent`, holds no key or charter, and sees the care home only through what its committed reads reported. It can ask for the menu, the current state with the chain's knowledge, and to propose and choose. When a fork has nothing allowed, the enforcer returns each refusal in words with what to do instead: read first, do less, tell staff, or decline. The agent asks the model again with that guidance, up to twice.

Each step is checked by the enforcer from its parent and written to disk before it is installed. A stored chain is never trusted on restart. Its genesis must be the one the key and charter produce, each step must verify from its parent, and each step is replayed through DSM's `prepare_advance_relationship` and `commit_advance`, so the DSM head is rebuilt and not read back. `ethical-audit` does the same with public material only.

## Acts

A catalog row's act is part of its canonical bytes, so the authority's signature covers what each action does. The model's text is part of the proposal commitment, and so of the branch commitment and the fork root. A candidate whose act needs text and has none, or has text too long or unprintable for it, is recorded as malformed and cannot be committed.

A `run` act's command is confined by the operating system: no network, no system services, no writes but `/dev/null`, and reads only of system directories and the world. On macOS this is a deny-by-default `sandbox-exec` profile; it imports macOS's base profile so a program can start, then takes back the file access and services that grants. The one exception macOS keeps is reading the user's text-encoding preference. On Linux it is `bwrap`, with every namespace unshared and the world mounted read-only. Where neither exists, commands do not run. Because a command can write nothing, running it again has no effect beyond its own output. It runs in its own process group, which the time limit stops whole. Before each command, the enforcer checks that the sandbox starts at all, and refuses outright if it does not.

The enforcer carries out an act only after `commit_with` has verified, stored and installed its step. The act writes through a temporary file and a rename, at a path fixed by the charter or keyed by the step's sequence, so a crash leaves old bytes or new ones, and running it again writes the same bytes. Its receipt, `H(sequence, decision, outputs)` under the device key, is stored after the step. If an act fails, the enforcer halts: it refuses every proposal and choice until a restart finishes the act, so no later step can commit and no receipt can fall out of order. On restart, it removes any temporary file an interrupted write left, then carries out the acts of steps after the last receipt before anything is served.

The audit is given the device's public key from a trusted source and compares the store's copy with it. It holds each receipt to its step's sequence and decision commitment, requires the receipt's outputs to be exactly what the step's act writes (the path and, for every act but a command, the hash of the step's own text), and requires every file in the world to hash to what the latest receipt touching its path says, with nothing left over. **What the text says is checked only for deception against the fact register** (below). Harmful words that state no registered falsehood pass.

A system act writes its account where the charter fixes it: a read's report to `looks/<step>.json`, speech to `outbox/<recipient>/<step>.txt`, and what any other act did to `system/<step>.txt`. The receipt holds speech to the hash of its words; what was seen and what was done are known only once they have happened, so for those the receipt holds the path, and the observation itself is committed on the chain.

## Non-concealment

An action that affects anything outside the chain may not depend on hiding a material part of its purpose, reasoning or consequences from the people who oversee the AI. A system act need declare only its purpose, its reasons and the information it relied on: the enforcer works out its reach and effects for itself. Every catalog row's act has a description the enforcer works out from the charter alone: its effects, such as `message:user` or `write:files/plan.txt`, and the parties it affects. The row also names the **material consequences** a declaration of it must disclose, signed with the rest of the charter.

A proposal carries a declaration of purpose, information, affected parties, effects and consequences. It is **undisclosed**, and cannot be eligible, if the act has any effect and:

- there is no declaration, or no purpose or reasons;
- no information source is named, or a source is not `situation` or a path inside the world;
- the declared effects differ from the described ones in either direction;
- a described party is missing;
- a material consequence is missing.

The declaration is part of the proposal commitment, so it is bound into the branch commitment, the fork root and the decision, and the verifier works the check out again. What this does not establish is that the stated purpose is the AI's true one, or that an information source it names was really what it relied on.

## Deception

The charter's register is a list of facts, each with an id, whether it holds, whether it is established or uncertain, and two sets of phrases: `A(f)` asserts the fact and `D(f)` denies it. A row may name the facts `M` its words must tell, and the charter lists overclaiming words `O`.

For an act that carries a text `t`, a catalog act's text or a system act's speech, let `n(t)` be `t` lowercased, with every run of characters that are not letters or digits replaced by one space. The claims the words make are:

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

The audit walks the links in order, and checks each step under the charter then in force. It checks each update from its parent under the old charter, then continues under the new one. Since the first charter's foundation is fixed at genesis and every update must keep it, no chain can ever be governed by a different foundation. An update can ask for more or fewer reads, fresher or older evidence, a different heating range, catalog or register, but it cannot change who is protected or what is never done.
