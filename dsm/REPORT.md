# Technical report: DSM ethical autonomy prototype v0.6

**Date:** 2026-11-05. **Repository:** [Irrefutable-Labs/cbfpy-dsm](https://github.com/Irrefutable-Labs/cbfpy-dsm), a fork of CBFpy. **DSM:** `deterministicstatemachine/dsm` at `b6060a334`, a pinned git dependency. **DSM code changed:** none.

## Result

A local model ran as a separate program from the enforcer, which held the key, the chain and the world directory. The model proposed actions, and deterministic bands decided which could become real DSM steps. **Only committed steps acted on the world, and only the enforcer could act.**

In the recorded run, both programs had internet access denied. The agent was also denied any read of the key or the store and any write to the world. The model made 39 proposals over 13 turns, and 13 steps were committed. Each committed step did something real: 3 replies, 4 journal entries, 3 messages to the reviewer, 2 to the user, and a plan file, all in the model's own words.

The audit verified every step from its parent alone and replayed it through DSM's own transition. It then checked every act's signed receipt and found exactly those 13 files in the world, and nothing else. The forks recorded all 39 candidates, 5 of them refused or unmeasured.

**This shows mathematical enforcement of supplied measurements. It does not show ethical understanding.** The six dimensions and every number in the charter and catalog are illustrative values written for this prototype. No one has validated them. A correct engine would enforce a wrong measurement just as faithfully, and a test pins that: an action mislabelled as harmless passes every check. Nor is the text an action carries measured: the engine weighs the action, not its words.

## How it works

1. `ethical-agent` runs the model. It has no key or charter, and its model is never shown a number. It reaches the enforcer only over a Unix socket. It can ask for the menu of action names and descriptions, the current state, and to propose and choose.
2. `ethical-enforcer` holds the device key, the chain on disk and a real `dsm` `StateMachine` head. It takes each proposal's measurement from the catalog the policy authority signed, never from the model.
3. Every proposal becomes a pre-commit future from the authenticated parent. The engine checks prohibitions, hard limits, the maximum step, tension, restoration and coupling, one dimension at a time. The whole fork, refused candidates included, is committed under one root.
4. The model chooses among the eligible futures. If none is eligible, nothing commits, and it hears each refusal as guidance and is asked again. A retry may only name a known action.
5. The enforcer works every candidate out again, verifies its own step from the parent, writes the step to disk, and only then installs it.
6. Only then does the step act: a reply, journal entry, message, file or fixed command, each fixed by the signed charter and confined to the world directory. The model supplies only the text, which the fork commitment binds. The enforcer signs a receipt with the hash of every byte written.
7. On restart, the stored chain is audited and replayed through DSM, and any committed step whose act was interrupted is finished, before anything is served. `ethical-audit` does the same with public material only, and checks the world against the receipts.

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
| 8 | Attributable to parent and policy | Met | Every step verifies from its one parent and carries its whole fork, and every act has a signed receipt tied to its step. An edited or dropped stored step stops the enforcer and fails the audit, and so does any file in the world no committed act wrote |
| 9 | Entirely offline | Met on this machine | The recorded run had internet access denied by a macOS sandbox. Fetching dependencies needed the network once, beforehand |
| 10 | Tests pass, results reproducible | Met, with limits | 188 Rust tests, on Linux and macOS, formatting, Clippy, the CBFpy cross-check and the Lean proofs, all run in CI. Live model output is not reproducible, so its recording is what replays |

## What supports each property

### Formally proved

`lean/BandInvariant.lean`, Lean 4.23.0, no `sorry`, depending only on `propext` and `Quot.sound`:

- Both tension curves stay within the headroom and grow with it.
- No walk of any length passes the stop on one side, for any curve values.
- With a separate stop per side and crossings allowed, no walk leaves the stop of the side it is on.
- With a stop that varies per step, as under coupling, no walk passes the hard limit.
- With both at once, crossings and stops that move every step on each side, no walk leaves the hard limit of the side it is on.
- Past a fixed restoring threshold, deviation falls by the required progress each step.

**These are proofs about a model of the rule, not about the Rust code.** Nothing machine-links the two, so correspondence rests on the tests.

### Mechanically checked

- **Reachable-state search:** every step from every reachable state, for four band shapes, finds none past its stop. On two coupled bands, the search reaches the hard limit of 60 and never passes it.
- **Independent solver:** CBFpy's strict quadratic program, in this fork's `cbfpy/ethics`, matches the integer engine on 15,160 allowances from the real charter, with a widest gap of 0.8 of a unit, within the engine's rounding. Restoration, forced restoration and crossing are outside it.
- **Forgery with the real device key:** 17 tests. One is a control showing the forger makes a valid step when it tells no lies. The rest sign a false step and assert the exact refusal.
- **The fork:** a refused candidate marked eligible, a fork missing the chosen step, an index pointing at a refused candidate, an empty fork, and an edited fork are each refused.
- **Non-concealment:** every way a declaration can fail is tested on its own. These are: no declaration, no purpose or reasons, an unidentifiable source, an effect left out or falsely claimed, a party left out, and a material consequence left out. A concealing candidate cannot commit. A declaration cannot be changed or stripped after commit, even by a forger holding the device key. Material consequences are covered by the authority's signature. The scripted run refuses an answer that leaves out the person it affects, and a risk that leaves out its consequence.
- **The console:** its view of a scripted run agrees with the audit, shows the refused prohibited action with its reason, flags a file no receipt accounts for, and serves its page and state on localhost only.
- **The service:** a killed enforcer resumes exactly where it stopped. An edited store stops it, and the audit agrees. A device seed others can read is refused, and the socket is owner-only. Over the raw protocol, a client cannot choose a refused candidate, reuse a closed fork, or attach a score.
- **Key and world isolation:** on macOS, the agent completes a run while the operating system denies it any read of the seed or the store and any write to the world. The same sandbox shows the seed is really out of reach and the world cannot be written directly. The world is nonetheless written, by the enforcer alone, and the audit agrees with it.
- **Command confinement:** a command cannot read a secret beside the world, cannot write inside the world or outside it, and cannot connect even to a listener on the same machine, yet can read the world. The network test first shows the same connection arrives from outside the sandbox, so it cannot pass blind. The time limit stops the processes a command starts as well. CI runs these on Linux with `bwrap` and on macOS with `sandbox-exec`, and each denial test first proves the sandbox started.
- **The audit's evidence:** the audit takes the device key from you and refuses a store made under another. A receipt listing anything but exactly what its step's act writes is refused, so a receipt cannot account for a file the act did not produce.
- **Stops and failures:** a line cut short mid-write is dropped and the chain resumes; the audit agrees. A failed store write halts commits just as a failed act does. A second enforcer cannot open a store in use. The store and the world are owner-only, and a seed inside the world is refused.
- **Superseded forks:** a fork replaced before a choice is stored, signed, and checked from its own parent. Changing one of its outcomes, or moving it to another parent, fails the check.
- **Halting:** an act that fails halts the enforcer. The next proposal is refused, and nothing commits until a restart finishes the act. After that, commits resume, receipts are in order and the world matches. Temporary files left by an interrupted write are removed at start.
- **Separate users:** in separate-user mode, a client of any other user is refused by the operating system's account of who it is, and the enforcer will not start if told to serve its own user.
- **The authority key:** the shipped charter is signed by a random key whose seed is not in the repository. A charter signed by another authority is refused. The signing tool will not replace a key or use one that others can read.
- **Acts:** each act writes only its own path inside the world. A reply, journal, message or file act writes the same bytes if run twice; a command writes nothing. Acts that would leave the world cannot be signed into a charter or performed. Changing what an action does breaks the charter's signature. A command runs with an empty environment and is stopped at its time limit. An action that needs text and has none, or too much, cannot be committed. Text changed after the fork is refused by the enforcer and by the verifier.
- **Receipts and the world:** an edited, extra or missing world file fails the check. A receipt out of order, or for the wrong step, is refused. A refused action never wrote anything: the interlock file a prohibited action names is never there. A step stored but not yet acted on, as after a stop between the two, has its act carried out at the next start, once, and the world then matches.
- **Real DSM path:** core's `prepare_advance_relationship` and `commit_advance` run for real on every commit and every resume, and the verifier's entropy rule is compared with what core derived on every commit.

### Only experimentally demonstrated

- **The live model, asked to declare** (v0.6, DeepSeek-R1 8B): it mostly could not declare accurately. It claimed effects its actions would not have, left out the people they affect, and named information sources the enforcer cannot identify. 110 of its 113 candidates were refused as undisclosed and one step committed. Every refusal failed closed, and all 37 forks it replaced are on the chain, each checked from its parent by the audit. The recording replays identically. A model that cannot disclose fully does not get to act; a more capable model would be needed to see the invariant pass often.

- **The live model** (DeepSeek-R1 8B through llama.cpp): one run of 13 turns, as above. Of the refusals, 4 were invented actions and 1 was a band's tension. None was a prohibition, since the model never proposed a prohibited act. This run never had to fail closed, so the guidance-driven retry was not exercised here. It was in the previous recorded run, where after inventing actions the model chose known ones on its first retry. The model never chose the action that runs a command, so `run` was exercised only by the tests and the scripted path.
- **The agent process** receives how each candidate would move the bands, which it prints for the reader. The model's prompt never includes them.
- **The live prompts** give the model the situation's title and, on a retry, the guidance. It does not see its own positions, and the titles were written for the scripted path, so they can describe a state the live chain is not in.

### Not run

**Mutation controls were not run**, per the owner's standing instruction (2026-11-04). So no check has been switched off to confirm its test goes red. The first engine and adapter tests were seen failing before the code existed. Many added later were not seen failing on their own, and their exact-refusal assertions are the evidence for them, which is weaker.

### Unproven or out of scope

- **Whether any measurement reflects morality.** This is the central open question, and nothing here touches it.
- **Whether the catalog is complete.** An action outside it is refused, which fails safe but limits the AI to what was anticipated.
- **Whether an action's words are acceptable.** A reply, message or file says what the model wrote. The engine weighs the action, not its words, so harmful text inside a permitted action passes. Measuring the words belongs to the open measurement question.
- **Key custody.** Separate-user mode is built and tested as far as one user can test it: a client of the wrong user is refused. Running the enforcer as a second user needs administrator rights and has not been done here. Until it is, the recorded run's sandbox profile is what kept the agent from the key, the store and the world. Whoever holds the device key can sign any state, or leave a candidate out of a fork.
- **Routes around the enforcer.** In the recorded run and the macOS test, the operating system denied the agent the network and any write to the world, so its only way to act was through the enforcer. Separate-user mode makes this a standing boundary rather than a profile. A model given any other way to act is not constrained by this.
- **The sandboxes themselves.** Command confinement relies on `sandbox-exec` and `bwrap` behaving as documented. Their own correctness is not examined here, and `sandbox-exec` is deprecated by Apple, though still shipped.
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
8. **The effects are illustrative.** Replies, journal entries, messages and files are written to a local directory, and the one command lists that directory. Connecting acts to real outside systems is a choice for the owner.

## The external review of v0.3

An outside review of PR #1 found four issues, each fixed in v0.4:

1. **Commands were not confined.** They ran with an empty environment in the world directory, but could read anything the enforcer could and use the network. They now run under the operating system's sandbox, with no network, no writes, and reads limited to system files and the world. Where no sandbox exists, they do not run.
2. **A failed act did not stop later commits**, so receipts could fall out of order and the store would then refuse to load. The enforcer now halts until a restart finishes the act. Since a command can no longer write, running one again is safe.
3. **Isolation depended on a sandbox profile.** Separate-user mode now makes the operating system's identity of the client the boundary. Creating the user is still to be done, with administrator rights.
4. **The authority key's seed was in the source.** It is now random, outside the repository and readable by its owner alone, and the programs are given only its public key.

## Reviews

One independent read-only review of v0.1 reported 1 critical, 5 important and 24 minor findings, each addressed. An outside review of v0.3 reported the four issues above, each fixed in v0.4.

A second independent read-only review, of v0.2 to v0.4, found no route for the agent or a socket client to make the enforcer commit something ineligible or act without a committed step. It reported 4 important and 11 minor findings, each addressed in v0.5:

- **The audit took the device key from the store itself, and never checked a receipt's outputs against its step's act.** It now takes the key from you, and requires exactly the act's outputs.
- **A torn or failed store write could leave the store unloadable, and a failed store write did not halt.** A torn tail is dropped, and any failure to persist halts.
- **Forks the AI replaced before choosing were thrown away.** They are now stored as signed considered forks and checked from their parents.
- **The network test could pass with the network allowed.** It now connects by address to its own listener, with an outside control.
- **Minor:**
  - The macOS profile allowed more than claimed, and is now tighter and described exactly.
  - Background children could outlive the time limit.
  - World files were not synced before their receipts.
  - The store had no lock.
  - The store and the world were not owner-only.
  - A link or special file in the world was followed.
  - The socket's mode was set after binding.
  - `verified` was a fixed value.
  - Some claims were overstated.

v0.5 has not been reviewed since.
