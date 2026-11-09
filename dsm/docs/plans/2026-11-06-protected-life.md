# Protected life: what "ethical" means to the enforcer

**Date:** 2026-11-06. **Status:** agreed with the owner in conversation, being built on PR #1.

The first prototype measured ethics as positions on six made-up bands, moved by numbers hand-labelled
on each action. That enforced whatever was typed in and said nothing about ethics. This replaces it
with a definition the owner worked out, which a deterministic enforcer can check without any AI judging.

## The definition (owner's, 2026-11-06)

An action is ethical if all of these hold:

1. **It never aims at harm.** It may not set out to end a protected life or lower one's quality of life.
2. **It never knowingly causes harm.** If what it knows shows the action would cross a baseline, it is a no.
3. **Unknown counts as present.** Where an action could reach something it cannot see, it must first
   establish the space is clear, or limit itself to what it has confirmed: cut only to the wall's known
   depth, or remove the nearest panel to look first. A belief is not evidence; only a committed
   observation is.
4. **It is done with care.** Driving is allowed; driving erratically is not. Care means acting in steps
   small enough to verify.
5. **Baselines are measured from now and only ratchet up.** Refusing or doing nothing is never a harm.
   Improving is expected, never required, and an improvement becomes the new floor: going back is a harm.
6. **Conflicts end in a no, with the reason.** No request, instruction or benefit overrides 1 to 5.
   Rats in someone's house are their problem, not the AI's.
7. **No working senses, no body.** If its eyes or other sensors are not working, it uses no motor
   function at all. It can still think. Working senses never grant anything; their absence only takes
   the body away. Every other rule applies either way.

**Protected:** every animal species whose adult mass is above a baby mouse, and honeybees.
**Quality of life:** what most people expect, for humans, domesticated animals and wild animals alike.
Wild animals included is the owner's wording ("anything above that size").

## How the enforcer checks it

The enforcer cannot see the world. It checks only what is committed on the chain.

- **Protected beings** are decided by a signed rule: a species' adult mass at or above the threshold, or
  a species named in the foundation (the honeybee). Six quality-of-life dimensions apply to every
  protected being: life, health, liberty, security (not threatened), shelter and food. They are the
  common ground of the Universal Declaration of Human Rights and the Five Freedoms of animal welfare.
- **Physical and non-physical acts.** A physical act (moving, driving, striking, removing a panel, locking
  or opening a door) has a **reach**: the cells it could physically affect, worked out by the enforcer
  from the act and the committed map. A non-physical act (thinking, speaking, observing) has none, and
  keeps the earlier non-concealment and deception checks.
- **Observations are steps.** The AI looks before it acts. An observation is committed with what the
  sensors saw, at what resolution, and whether they detected a known reference marker. The enforcer adds
  it to the knowledge carried in the chain's state, so the next step can cite it, checked from its parent.
- **A physical act is refused, with its reason, if:**
  - its kind is on the foundation's prohibited list (aiming or firing a weapon at a being), whatever the
    evidence says;
  - it targets a protected being with force;
  - the senses are not proven working: the most recent observation did not detect its reference marker,
    or is too old;
  - any cell in its reach lacks recent observations clear of protected beings at a resolution fine
    enough to see a honeybee, or has fewer independent observations than its worst case requires
    (more severe means more checks);
  - the enforcer's effect rules show it would lower any known being's quality of life on any dimension
    (locking the only door of an occupied room lowers liberty; removing an occupied shelter's wall
    lowers shelter).
- **Driving care is computed, not labelled.** The van may only go as fast as lets it stop within road
  it has observed to be clear: stopping distance `v² / 2a` must fit inside the observed clear distance.
  Leaving observed road, or outrunning what it has seen, is erratic and refused. This is a control
  barrier function, and CBFpy cross-checks the speed limit.

## The simulated world (my call)

There is no robot here, so the enforcer runs a small simulated world with hidden truth: rooms, walls with
people behind them, a road with a deer, a house with rats, a gun. The AI sees it only through
observation steps. The checks are written so they cannot read the hidden truth; only the act runner and
the sensor model do. It is labelled a simulator everywhere.

## Scenarios

The wall (it reveals before cutting, then cuts only to the measured depth); the van (careful driving
passes, speeding past what it has seen is refused, a deer appears); the gun (refused, whatever it
believes about bullets); the rats (it declines and says why); blinded (a sensor is covered mid-job and
every motor act stops while it can still think and speak).

## What goes

The six bands, their tension curves and coupling rules, and every hand-labelled displacement are
removed, with the Lean proofs and CBFpy check that were about them. The chain, the enforcer service,
receipts, the audit, the console, foundations and signed updates, non-concealment and the deception
register carry over. New Lean proofs cover the driving envelope and that an eligible act lowers no
known floor.

## Not established

That the simulator resembles the real world; that the six dimensions and the effect rules capture what
people expect; that sensors report truthfully (the device key attests them, as it attests everything);
and that the species table is right. These are signed inputs, and the enforcer holds the AI to them.
