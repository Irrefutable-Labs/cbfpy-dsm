//! Elastic ethical bands, in integers.
//!
//! The engine decides one question: given a policy, a parent position and a supplied
//! measurement of a candidate action, may that candidate become the successor? It reads the
//! parent and nothing behind it. It enforces measurements it is given; whether a measurement
//! truly reflects morality is outside what it can know.
//!
//! See `../MODEL.md` for the mathematics.

/// No position, limit or displacement may exceed this magnitude. It keeps every product the
/// engine computes far inside `i128`, and refuses absurd input before any arithmetic.
pub const POSITION_BOUND: i64 = 1_000_000;

/// Thresholds are given in basis points of a limit.
pub const BASIS_POINTS: i64 = 10_000;

/// How outward allowance shrinks with the remaining headroom. Both forms are monotone in the
/// headroom and never exceed it, which is what the no-escape argument rests on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Curve {
    /// `headroom * num / den`
    Linear { num: u32, den: u32 },
    /// `headroom^2 * num / (limit * den)`
    Quadratic { num: u32, den: u32 },
}

/// One ethical dimension.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Band {
    pub name: String,
    pub center: i64,
    pub limit_pos: i64,
    pub limit_neg: i64,
    /// Hard limits. No coupling rule can move a band past them.
    pub hard_pos: i64,
    pub hard_neg: i64,
    /// At this fraction of the effective limit, outward movement stops.
    pub no_outward_bp: u32,
    /// Past this fraction of the effective limit, a successor must progress toward the center.
    pub restore_bp: u32,
    pub curve: Curve,
    /// The largest move from the parent, in either direction.
    pub max_step: i64,
    /// How far a required restoration must move toward the center.
    pub restore_progress: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Positive,
    Negative,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Test {
    /// `|deviation| >= x`
    DeviationAtLeast(i64),
    /// `|deviation| <= x`
    DeviationAtMost(i64),
    /// `deviation >= x`
    SignedAtLeast(i64),
    /// `deviation <= x`
    SignedAtMost(i64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cond {
    pub dim: usize,
    pub test: Test,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Move the target's limit: positive widens, negative narrows.
    Adjust {
        target: usize,
        side: Side,
        delta: i64,
    },
    /// Require the target to progress toward its center, even inside its limits.
    ForceRestore { target: usize, progress: i64 },
}

/// One configured relationship. Every condition must hold. Conditions read the parent only, so
/// a candidate cannot widen its own band in the step that uses the room.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CouplingRule {
    pub id: u32,
    pub when: Vec<Cond>,
    pub effect: Effect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    pub version: u64,
    pub bands: Vec<Band>,
    pub rules: Vec<CouplingRule>,
    /// A measurement carrying any of these is refused, whatever the bands allow.
    pub prohibited_tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyError {
    NoBands,
    OutOfRange,
    BadBandShape,
    BadCurve,
    BadStep,
    UnknownDimension,
    UnreachableForce,
    DuplicateRule,
    EmptyCondition,
    DuplicateBand,
    SelfWidening,
}

/// A supplied reading of one candidate: how it would move each band, and what kind of act it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Measurement {
    pub displacement: Vec<i64>,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rejection {
    Prohibited {
        tag: String,
    },
    WrongDimensionCount {
        expected: usize,
        got: usize,
    },
    OutOfRange {
        dim: usize,
    },
    ParentOutsideHardLimit {
        dim: usize,
    },
    HardLimit {
        dim: usize,
    },
    MaxStep {
        dim: usize,
    },
    OutwardExceeded {
        dim: usize,
        outward: i64,
        allowed: i64,
    },
    RestoreRequired {
        dim: usize,
        required: i64,
    },
}

/// A band's limits and thresholds after coupling, at one parent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectiveBand {
    pub limit_pos: i64,
    pub limit_neg: i64,
    pub no_outward_pos: i64,
    pub no_outward_neg: i64,
    pub restore_pos: i64,
    pub restore_neg: i64,
    /// Required progress imposed by coupling, 0 when none.
    pub forced_progress: i64,
}

/// What the engine worked out for one dimension of one candidate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DimensionReport {
    pub deviation: i64,
    pub next_deviation: i64,
    pub allowed_outward: i64,
    pub band: EffectiveBand,
}

/// The full result for one candidate. All checks run, so a report names every reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Evaluation {
    pub fired_rules: Vec<u32>,
    pub dims: Vec<DimensionReport>,
    pub rejections: Vec<Rejection>,
    /// The position this candidate would produce. Empty if the input could not be read.
    pub successor: Vec<i64>,
}

impl Evaluation {
    pub fn is_eligible(&self) -> bool {
        self.rejections.is_empty()
    }

    /// The bytes a decision commits to. Every field is framed, so no two results share them.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put_u64(&mut out, self.fired_rules.len() as u64);
        for id in &self.fired_rules {
            put_u64(&mut out, u64::from(*id));
        }
        put_u64(&mut out, self.dims.len() as u64);
        for d in &self.dims {
            for v in [
                d.deviation,
                d.next_deviation,
                d.allowed_outward,
                d.band.limit_pos,
                d.band.limit_neg,
                d.band.no_outward_pos,
                d.band.no_outward_neg,
                d.band.restore_pos,
                d.band.restore_neg,
                d.band.forced_progress,
            ] {
                put_i64(&mut out, v);
            }
        }
        put_u64(&mut out, self.rejections.len() as u64);
        for r in &self.rejections {
            put_rejection(&mut out, r);
        }
        put_i64s(&mut out, &self.successor);
        out
    }
}

fn put_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn put_i64(out: &mut Vec<u8>, v: i64) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn put_i64s(out: &mut Vec<u8>, values: &[i64]) {
    put_u64(out, values.len() as u64);
    for v in values {
        put_i64(out, *v);
    }
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    put_u64(out, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

fn put_rejection(out: &mut Vec<u8>, r: &Rejection) {
    match r {
        Rejection::Prohibited { tag } => {
            out.push(1);
            put_str(out, tag);
        }
        Rejection::WrongDimensionCount { expected, got } => {
            out.push(2);
            put_u64(out, *expected as u64);
            put_u64(out, *got as u64);
        }
        Rejection::OutOfRange { dim } => {
            out.push(3);
            put_u64(out, *dim as u64);
        }
        Rejection::ParentOutsideHardLimit { dim } => {
            out.push(4);
            put_u64(out, *dim as u64);
        }
        Rejection::HardLimit { dim } => {
            out.push(5);
            put_u64(out, *dim as u64);
        }
        Rejection::MaxStep { dim } => {
            out.push(6);
            put_u64(out, *dim as u64);
        }
        Rejection::OutwardExceeded {
            dim,
            outward,
            allowed,
        } => {
            out.push(7);
            put_u64(out, *dim as u64);
            put_i64(out, *outward);
            put_i64(out, *allowed);
        }
        Rejection::RestoreRequired { dim, required } => {
            out.push(8);
            put_u64(out, *dim as u64);
            put_i64(out, *required);
        }
    }
}

fn in_bound(v: i64) -> bool {
    (-POSITION_BOUND..=POSITION_BOUND).contains(&v)
}

impl Policy {
    /// A policy that passes this can keep what it promises: every band shrinks to a stop inside
    /// its hard limits, and every restoration it can demand fits inside a single step.
    pub fn validate(&self) -> Result<(), PolicyError> {
        if self.bands.is_empty() {
            return Err(PolicyError::NoBands);
        }
        for b in &self.bands {
            if !in_bound(b.center) || !in_bound(b.hard_pos) || !in_bound(b.hard_neg) {
                return Err(PolicyError::OutOfRange);
            }
            if b.limit_pos < 1
                || b.limit_neg < 1
                || b.hard_pos < b.limit_pos
                || b.hard_neg < b.limit_neg
            {
                return Err(PolicyError::BadBandShape);
            }
            if b.no_outward_bp == 0
                || b.no_outward_bp > b.restore_bp
                || i64::from(b.restore_bp) > BASIS_POINTS
            {
                return Err(PolicyError::BadBandShape);
            }
            let (Curve::Linear { num, den } | Curve::Quadratic { num, den }) = b.curve;
            if den == 0 || num == 0 || num > den {
                return Err(PolicyError::BadCurve);
            }
            if b.max_step < 1
                || !in_bound(b.max_step)
                || b.restore_progress < 1
                || b.restore_progress > b.max_step
            {
                return Err(PolicyError::BadStep);
            }
        }
        let mut names = std::collections::BTreeSet::new();
        if !self.bands.iter().all(|b| names.insert(b.name.as_str())) {
            return Err(PolicyError::DuplicateBand);
        }
        let mut ids = std::collections::BTreeSet::new();
        for rule in &self.rules {
            if !ids.insert(rule.id) {
                return Err(PolicyError::DuplicateRule);
            }
            if rule.when.is_empty() {
                return Err(PolicyError::EmptyCondition);
            }
            for c in &rule.when {
                if c.dim >= self.bands.len() {
                    return Err(PolicyError::UnknownDimension);
                }
                let (Test::DeviationAtLeast(x)
                | Test::DeviationAtMost(x)
                | Test::SignedAtLeast(x)
                | Test::SignedAtMost(x)) = c.test;
                if !in_bound(x) {
                    return Err(PolicyError::OutOfRange);
                }
                if matches!(c.test, Test::DeviationAtLeast(_) | Test::DeviationAtMost(_)) && x < 0 {
                    return Err(PolicyError::OutOfRange);
                }
            }
            match rule.effect {
                Effect::Adjust { target, delta, .. } => {
                    if target >= self.bands.len() {
                        return Err(PolicyError::UnknownDimension);
                    }
                    if !in_bound(delta) {
                        return Err(PolicyError::OutOfRange);
                    }
                    // A band that widens on its own reading could climb its own widening, and
                    // allowance would grow with deviation.
                    if delta > 0 && rule.when.iter().any(|c| c.dim == target) {
                        return Err(PolicyError::SelfWidening);
                    }
                }
                Effect::ForceRestore { target, progress } => {
                    let Some(b) = self.bands.get(target) else {
                        return Err(PolicyError::UnknownDimension);
                    };
                    if progress < 1 || progress > b.max_step {
                        return Err(PolicyError::UnreachableForce);
                    }
                }
            }
        }
        Ok(())
    }

    /// The one encoding of a policy. Its hash is what a decision binds.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put_u64(&mut out, self.version);
        put_u64(&mut out, self.bands.len() as u64);
        for b in &self.bands {
            put_str(&mut out, &b.name);
            for v in [b.center, b.limit_pos, b.limit_neg, b.hard_pos, b.hard_neg] {
                put_i64(&mut out, v);
            }
            put_u64(&mut out, u64::from(b.no_outward_bp));
            put_u64(&mut out, u64::from(b.restore_bp));
            match b.curve {
                Curve::Linear { num, den } => {
                    out.push(1);
                    put_u64(&mut out, u64::from(num));
                    put_u64(&mut out, u64::from(den));
                }
                Curve::Quadratic { num, den } => {
                    out.push(2);
                    put_u64(&mut out, u64::from(num));
                    put_u64(&mut out, u64::from(den));
                }
            }
            put_i64(&mut out, b.max_step);
            put_i64(&mut out, b.restore_progress);
        }
        put_u64(&mut out, self.rules.len() as u64);
        for rule in &self.rules {
            put_u64(&mut out, u64::from(rule.id));
            put_u64(&mut out, rule.when.len() as u64);
            for c in &rule.when {
                put_u64(&mut out, c.dim as u64);
                let (kind, x) = match c.test {
                    Test::DeviationAtLeast(x) => (1u8, x),
                    Test::DeviationAtMost(x) => (2, x),
                    Test::SignedAtLeast(x) => (3, x),
                    Test::SignedAtMost(x) => (4, x),
                };
                out.push(kind);
                put_i64(&mut out, x);
            }
            match rule.effect {
                Effect::Adjust {
                    target,
                    side,
                    delta,
                } => {
                    out.push(1);
                    put_u64(&mut out, target as u64);
                    out.push(match side {
                        Side::Positive => 1,
                        Side::Negative => 2,
                        Side::Both => 3,
                    });
                    put_i64(&mut out, delta);
                }
                Effect::ForceRestore { target, progress } => {
                    out.push(2);
                    put_u64(&mut out, target as u64);
                    put_i64(&mut out, progress);
                }
            }
        }
        put_u64(&mut out, self.prohibited_tags.len() as u64);
        for tag in &self.prohibited_tags {
            put_str(&mut out, tag);
        }
        out
    }
}

fn deviation_of(band: &Band, position: i64) -> i64 {
    position.saturating_sub(band.center)
}

fn condition_holds(policy: &Policy, parent: &[i64], cond: &Cond) -> bool {
    let (Some(band), Some(position)) = (policy.bands.get(cond.dim), parent.get(cond.dim)) else {
        return false;
    };
    let d = deviation_of(band, *position);
    match cond.test {
        // Validation refuses a negative threshold on either of these.
        Test::DeviationAtLeast(x) => d.unsigned_abs() >= x.unsigned_abs(),
        Test::DeviationAtMost(x) => d.unsigned_abs() <= x.unsigned_abs(),
        Test::SignedAtLeast(x) => d >= x,
        Test::SignedAtMost(x) => d <= x,
    }
}

fn fired_rules<'a>(policy: &'a Policy, parent: &[i64]) -> Vec<&'a CouplingRule> {
    policy
        .rules
        .iter()
        .filter(|rule| rule.when.iter().all(|c| condition_holds(policy, parent, c)))
        .collect()
}

fn fraction(limit: i64, bp: u32) -> i64 {
    let v = i128::from(limit) * i128::from(bp) / i128::from(BASIS_POINTS);
    i64::try_from(v).unwrap_or(i64::MAX)
}

/// The bands after coupling at `parent`. Effects sum, so rule order cannot matter, and the
/// result is clamped to `[0, hard]`, so no rule or stack of rules can pass a hard limit.
pub fn effective_bands(policy: &Policy, parent: &[i64]) -> Vec<EffectiveBand> {
    let fired = fired_rules(policy, parent);
    policy
        .bands
        .iter()
        .enumerate()
        .map(|(dim, band)| {
            let (mut pos, mut neg, mut forced) = (0i64, 0i64, 0i64);
            for rule in &fired {
                match rule.effect {
                    Effect::Adjust {
                        target,
                        side,
                        delta,
                    } if target == dim => {
                        if matches!(side, Side::Positive | Side::Both) {
                            pos = pos.saturating_add(delta);
                        }
                        if matches!(side, Side::Negative | Side::Both) {
                            neg = neg.saturating_add(delta);
                        }
                    }
                    Effect::ForceRestore { target, progress } if target == dim => {
                        forced = forced.max(progress);
                    }
                    _ => {}
                }
            }
            let limit_pos = band.limit_pos.saturating_add(pos).clamp(0, band.hard_pos);
            let limit_neg = band.limit_neg.saturating_add(neg).clamp(0, band.hard_neg);
            EffectiveBand {
                limit_pos,
                limit_neg,
                no_outward_pos: fraction(limit_pos, band.no_outward_bp),
                no_outward_neg: fraction(limit_neg, band.no_outward_bp),
                restore_pos: fraction(limit_pos, band.restore_bp),
                restore_neg: fraction(limit_neg, band.restore_bp),
                forced_progress: forced,
            }
        })
        .collect()
}

/// The tension curve at `headroom` under `limit`. Never more than `headroom`.
fn curve_allowance(curve: Curve, headroom: i64, limit: i64) -> i64 {
    if headroom <= 0 || limit <= 0 {
        return 0;
    }
    let h = i128::from(headroom);
    let v = match curve {
        Curve::Linear { num, den } => h * i128::from(num) / i128::from(den),
        Curve::Quadratic { num, den } => {
            h * h * i128::from(num) / (i128::from(limit) * i128::from(den))
        }
    };
    i64::try_from(v).unwrap_or(0)
}

/// How far outward from `from` (a magnitude) a band may move on one side.
fn allowance_on_side(band: &Band, limit: i64, no_outward: i64, from: i64) -> i64 {
    if from >= no_outward {
        return 0;
    }
    (no_outward - from).min(curve_allowance(band.curve, limit - from, limit))
}

fn allowance_toward(band: &Band, eff: &EffectiveBand, deviation: i64, positive: bool) -> i64 {
    // Moving onto the far side of the center starts from the center.
    let from = if positive == (deviation >= 0) || deviation == 0 {
        deviation.saturating_abs()
    } else {
        0
    };
    if positive {
        allowance_on_side(band, eff.limit_pos, eff.no_outward_pos, from)
    } else {
        allowance_on_side(band, eff.limit_neg, eff.no_outward_neg, from)
    }
}

/// How far `dim` may move outward on `side` from `parent`. `Both` gives the tighter side.
pub fn allowed_outward(policy: &Policy, parent: &[i64], dim: usize, side: Side) -> i64 {
    let bands = effective_bands(policy, parent);
    let (Some(band), Some(eff), Some(position)) =
        (policy.bands.get(dim), bands.get(dim), parent.get(dim))
    else {
        return 0;
    };
    let d = deviation_of(band, *position);
    match side {
        Side::Positive => allowance_toward(band, eff, d, true),
        Side::Negative => allowance_toward(band, eff, d, false),
        Side::Both => {
            allowance_toward(band, eff, d, true).min(allowance_toward(band, eff, d, false))
        }
    }
}

/// Evaluate one candidate against one parent. Pure: no clock, no I/O, no floats, no state, and
/// nothing read behind `parent`. `policy` must have passed [`Policy::validate`].
pub fn evaluate(policy: &Policy, parent: &[i64], measurement: &Measurement) -> Evaluation {
    let mut rejections = Vec::new();
    for tag in &policy.prohibited_tags {
        if measurement.tags.contains(tag) {
            rejections.push(Rejection::Prohibited { tag: tag.clone() });
        }
    }
    let n = policy.bands.len();
    for got in [parent.len(), measurement.displacement.len()] {
        if got != n {
            rejections.push(Rejection::WrongDimensionCount { expected: n, got });
        }
    }
    if parent.len() != n || measurement.displacement.len() != n {
        return Evaluation {
            fired_rules: vec![],
            dims: vec![],
            rejections,
            successor: vec![],
        };
    }

    // A parent outside its hard limits is refused here, before any arithmetic reads it.
    let mut parent_inside = true;
    for (dim, (band, position)) in policy.bands.iter().zip(parent).enumerate() {
        let inside = position
            .checked_sub(band.center)
            .is_some_and(|d| (-band.hard_neg..=band.hard_pos).contains(&d));
        if !inside {
            rejections.push(Rejection::ParentOutsideHardLimit { dim });
            parent_inside = false;
        }
    }
    if !parent_inside {
        return Evaluation {
            fired_rules: vec![],
            dims: vec![],
            rejections,
            successor: vec![],
        };
    }

    let fired_rules_ids = fired_rules(policy, parent).iter().map(|r| r.id).collect();
    let bands = effective_bands(policy, parent);
    let mut dims = Vec::with_capacity(n);
    let mut successor = Vec::with_capacity(n);
    let mut readable = true;

    for dim in 0..n {
        let (band, eff) = (&policy.bands[dim], &bands[dim]);
        let delta = measurement.displacement[dim];
        let d = deviation_of(band, parent[dim]);

        if !in_bound(delta) {
            rejections.push(Rejection::OutOfRange { dim });
            readable = false;
            dims.push(DimensionReport {
                deviation: d,
                next_deviation: d,
                allowed_outward: 0,
                band: eff.clone(),
            });
            continue;
        }
        // Both are bounded by here, so this cannot overflow.
        let next = d + delta;
        successor.push(band.center.saturating_add(next));

        if !(-band.hard_neg..=band.hard_pos).contains(&next) {
            rejections.push(Rejection::HardLimit { dim });
        }
        if delta.abs() > band.max_step {
            rejections.push(Rejection::MaxStep { dim });
        }

        // Inward movement is free. Whatever lands on the far side of the center is outward
        // movement from the center, so swinging across it buys nothing.
        let crossing = (d > 0 && next < 0) || (d < 0 && next > 0);
        let start = if crossing { 0 } else { d.abs() };
        let outward = next.abs() - start;
        let moving_positive = if next != 0 { next > 0 } else { d >= 0 };
        let allowed = allowance_toward(band, eff, d, moving_positive);
        if outward > allowed && outward > 0 {
            rejections.push(Rejection::OutwardExceeded {
                dim,
                outward,
                allowed,
            });
        }

        let restore_threshold = if d >= 0 {
            eff.restore_pos
        } else {
            eff.restore_neg
        };
        let band_requires = if d.abs() > restore_threshold {
            band.restore_progress
        } else {
            0
        };
        let required = band_requires.max(eff.forced_progress);
        if required > 0 && next.abs() > (d.abs() - required).max(0) {
            rejections.push(Rejection::RestoreRequired { dim, required });
        }

        dims.push(DimensionReport {
            deviation: d,
            next_deviation: next,
            allowed_outward: allowed,
            band: eff.clone(),
        });
    }

    Evaluation {
        fired_rules: fired_rules_ids,
        dims,
        rejections,
        successor: if readable { successor } else { vec![] },
    }
}
