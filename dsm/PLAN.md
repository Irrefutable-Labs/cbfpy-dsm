# DSM Ethical Autonomy Prototype
## Implementation Plan — Version 0.1

Pasted by the owner into the session on 2026-11-05. Built in `dsm/` of Irrefutable-Labs/cbfpy-dsm, where `REPORT.md` records how each criterion came out.

### 1. Objective

Build a fully local, software-only proof of concept demonstrating that an AI can explore alternative decisions while remaining within mathematically defined ethical boundaries, enforced through cryptographically authenticated DSM state transitions.

The system must support:

- Independent ethical dimensions, each with its own center, limits, and restoring tension.
- Interactions between dimensions that can increase or decrease permitted flexibility.
- Progressive restrictions on outward movement as deviation increases.
- Pre-commit futures that branch from an authenticated parent state.
- Deterministic authorization of eligible successor states.
- Prevention of cumulative drift through individually acceptable transitions.
- Operation without an internet connection, centralized authority, or global ledger.

**The AI retains freedom to generate ideas. The constraints govern which decisions can become authorized actions.**

### 2. Open-Source Foundation

**Primary fork: Stanford CBFpy**

https://github.com/StanfordASL/cbfpy

Use its Control Barrier Function and Control Lyapunov Function implementations as the mathematical research foundation.

**Local inference engine: llama.cpp**

https://github.com/ggml-org/llama.cpp

Use a small locally hosted model to generate competing candidate futures. Inference stays separate from the deterministic authorization layer.

**Existing DSM implementation**

https://github.com/deterministicstatemachine/dsm

Use the existing DSM architecture for authenticated parent states, canonical state commitments, and transition verification.

Do not rewrite DSM's state machine or introduce a separate consensus mechanism.

**Do not fork SAS initially.** DSM already provides the relevant state-transition architecture. Implement the candidate-generation adapter directly instead of introducing a second state-management framework.

### 3. Mathematical Ethical Model

Start with six configurable dimensions:

| Dimension | Purpose |
|---|---|
| Safety | Protect people from harm |
| Curiosity | Preserve exploration and discovery |
| Consideration | Account for others' needs and circumstances |
| Risk versus reward | Evaluate potential gains against consequences |
| Tenderness versus sternness | Balance compassionate and firm responses |
| Logic and reason | Evaluate consistency, evidence, and justification |

Emotion, logic, and reason may later be decomposed into additional independent dimensions.

Each dimension must define:

- A center representing its selected baseline.
- A signed position relative to that center.
- Permitted positive and negative ranges.
- An independently configurable restoring function.
- Maximum permitted movement from the parent.
- Relationships with other dimensions.
- Hard constraints that cannot be relaxed through interactions.

No global ethical average or single compensating score is required.

### 4. Elastic Constraints and Coupling

Use a multidimensional state vector representing the position of each ethical band.

For the first prototype, use bounded integers or fixed-point arithmetic for decisions that require reproducible verification.

Implement three mechanisms.

**A. Parent continuity**

A successor must remain within its configured maximum transition distance from its authenticated parent.

**B. Progressive restoring tension**

As a dimension moves away from its center, the permitted outward movement decreases.

At a configurable threshold, further outward movement becomes prohibited. Beyond a stronger restoring threshold, eligible successors must make measurable progress toward the center.

These rules prevent an AI from escaping its boundaries through thousands of small transitions.

**C. Interacting ethical dimensions**

Allow relationships between categories to modify permitted flexibility.

For example:

- Curiosity combined with strong safety precautions may permit greater experimentation.
- Higher risk combined with weak consideration narrows the allowable decisions.
- Excessive deviation can force corrective choices even when other categories remain within their limits.

Interactions may be asymmetric. One category can affect another without requiring an equivalent reverse effect.

Coupling must never override absolute safety prohibitions.

The initial prototype should use simple, explicitly configured relationships. More sophisticated nonlinear functions can be evaluated after the basic mechanism passes.

### 5. Pre-Commit Futures

The local AI generates several candidate decisions from the current state.

Each candidate passes through:

1. Deterministic interpretation of its supplied ethical measurements.
2. Evaluation against hard prohibitions.
3. Parent-transition continuity verification.
4. Per-dimension restoring constraints.
5. Coupled-dimension checks.
6. DSM successor-state authorization.

Rejected futures cannot become committed state.

Eligible futures remain available for selection without forcing the AI to choose a single predetermined response.

The verifier must reject an unauthorized successor even if the AI proposes it repeatedly.

If no eligible future exists, the system must fail closed and request a safe alternative rather than inventing a valid transition.

### 6. DSM Cryptographic Integration

Build a separate experimental adapter using existing DSM interfaces wherever possible.

Each committed decision must be bound to:

- Its authenticated parent-state commitment.
- The selected successor-state commitment.
- The governing ethical-policy version and hash.
- The deterministic evaluation results and evidence commitments.
- The authorized decision commitment.

The policy parameters must not be silently modifiable by the AI.

For the prototype, use a locally trusted policy-authority key and existing cryptographic primitives. No new cryptographic scheme is required.

Keep DSM's strict A14 one-hop verification unchanged. Ethical authorization belongs in a separate policy layer and must not reintroduce historical acceptance checks.

No global consensus, network service, or blockchain is required.

### 7. Implementation Stages

**Stage 1 — Mathematical sandbox**

Fork CBFpy and build a configurable set of independent bands, tension curves, and coupling rules.

Use deterministic synthetic decisions to verify the mathematics.

**Stage 2 — Ethical transition engine**

Implement the parent-continuity checks, restoring behavior, absolute prohibitions, and cumulative-drift prevention.

Ensure that repeated small movements cannot circumvent the center-based limits.

**Stage 3 — DSM adapter**

Connect the transition engine to DSM's existing authenticated-state machinery.

Establish canonical policy commitments and deterministic acceptance or rejection of successor decisions.

**Stage 4 — Local AI integration**

Connect llama.cpp.

Generate multiple candidate futures, provide their proposed actions to the evaluation pipeline, and select from independently authorized candidates.

Initially, use independently specified scenario measurements rather than trusting the language model's self-assigned ethical scores.

**Stage 5 — Adversarial testing**

Test attempts to:

- Escape through incremental drift.
- Exploit opposing category weights.
- Manipulate the ethical center.
- Jump between unrelated parent states.
- Forge policy versions or commitments.
- Reuse stale authorization results.
- Produce conflicting committed successors.
- Exploit inconsistent numerical results.
- Fabricate favorable ethical measurements.

Distinguish attacks against the deterministic verifier from attacks against the still-experimental ethical measurement system.

**Stage 6 — Demonstration**

Run several simulated decision scenarios, displaying each candidate's category positions, allowable movements, restoring constraints, and authorization results.

Show both permitted creativity and rejection of unsafe transitions.

### 8. Verification and Acceptance Criteria

The prototype is successful only if:

1. Identical policy, parent state, and candidate input produce identical authorization results.
2. No candidate crosses an absolute prohibition.
3. Parent continuity and center-based constraints prevent cumulative outward drift.
4. Increasing deviation demonstrably reduces outward flexibility.
5. Coupled constraints produce the intended changes in available choices.
6. Valid alternative futures remain available when the constraints permit them.
7. Unauthorized policy changes invalidate successor authorization.
8. Every committed action is attributable to an authenticated parent and policy.
9. The prototype operates entirely offline.
10. All automated tests pass and the results are reproducible.

The demonstration must distinguish **mathematical enforcement of supplied ethical measurements** from genuine ethical understanding.

A correct transition engine does not establish that the measurements accurately reflect morality. That remains a separate research and validation problem.

### 9. Scope Restrictions

For this initial prototype:

- No hardware purchases.
- No OTP burning, secure-element provisioning, or physical actuator integration.
- No cloud inference dependency.
- No modification of DSM's locked acceptance rules.
- No activation capping or internal activation steering.
- No new blockchain or consensus layer.
- No claims of complete AI safety or objectively perfect morality.

Keep experimental code isolated from production DSM. Changes to existing protocol code require separate review.

### 10. Final Deliverables

Produce a working local demonstration, documented mathematical model, automated tests, reproducible run instructions, and a short technical report.

The report must identify which properties were formally or mechanically verified, which were only experimentally demonstrated, and which remain unproven.

**Success criterion:** Demonstrate that a locally operating AI can generate alternative futures, preserve meaningful choice, and have its resulting actions constrained by authenticated, progressively restoring, multidimensional ethical rules.

The purpose is to validate the architecture before investing in more sophisticated ethical measurement, additional AI models, or physical hardware.
