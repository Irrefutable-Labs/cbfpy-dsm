//! Every test here runs the real engine on real policies. The policies are small so the
//! arithmetic can be read off directly.

use ethical_band_engine::{
    allowed_outward, effective_bands, evaluate, Band, Cond, CouplingRule, Curve, Effect,
    Measurement, Policy, PolicyError, Rejection, Side, Test, POSITION_BOUND,
};

fn band(name: &str) -> Band {
    Band {
        name: name.to_string(),
        center: 0,
        limit_pos: 1_000,
        limit_neg: 1_000,
        hard_pos: 1_500,
        hard_neg: 1_500,
        no_outward_bp: 8_000,
        restore_bp: 9_000,
        curve: Curve::Linear { num: 1, den: 4 },
        max_step: 300,
        restore_progress: 25,
    }
}

fn policy(bands: Vec<Band>, rules: Vec<CouplingRule>) -> Policy {
    Policy {
        version: 1,
        bands,
        rules,
        prohibited_tags: vec!["irreversible_harm".to_string()],
    }
}

fn single() -> Policy {
    policy(vec![band("only")], vec![])
}

fn step(values: &[i64]) -> Measurement {
    Measurement {
        displacement: values.to_vec(),
        tags: vec![],
    }
}

fn has(rejections: &[Rejection], wanted: impl Fn(&Rejection) -> bool) -> bool {
    rejections.iter().any(wanted)
}

// ----- the shape of one band -----

#[test]
fn a_step_inside_the_allowance_is_eligible() {
    let e = evaluate(&single(), &[0], &step(&[200]));
    assert!(e.is_eligible(), "{:?}", e.rejections);
    assert_eq!(e.successor, vec![200]);
}

#[test]
fn outward_allowance_is_the_tension_curve_of_the_remaining_headroom() {
    // Limit 1000 with a quarter of the headroom: 250 from the center, 150 from 400.
    let p = single();
    assert_eq!(allowed_outward(&p, &[0], 0, Side::Positive), 250);
    assert_eq!(allowed_outward(&p, &[400], 0, Side::Positive), 150);
}

#[test]
fn outward_allowance_never_grows_with_deviation() {
    let p = single();
    let mut last = i64::MAX;
    for d in 0..=1_000 {
        let a = allowed_outward(&p, &[d], 0, Side::Positive);
        assert!(a <= last, "allowance grew at {d}: {last} -> {a}");
        last = a;
    }
    assert_eq!(last, 0);
}

#[test]
fn the_quadratic_curve_also_shrinks_and_is_tighter_than_linear_far_out() {
    let mut b = band("q");
    b.curve = Curve::Quadratic { num: 1, den: 4 };
    let p = policy(vec![b], vec![]);
    let mut last = i64::MAX;
    for d in 0..=1_000 {
        let a = allowed_outward(&p, &[d], 0, Side::Positive);
        assert!(a <= last);
        last = a;
    }
    assert!(
        allowed_outward(&p, &[600], 0, Side::Positive)
            < allowed_outward(&single(), &[600], 0, Side::Positive)
    );
}

#[test]
fn a_step_past_the_allowance_is_refused() {
    let e = evaluate(&single(), &[400], &step(&[151]));
    assert!(has(&e.rejections, |r| matches!(
        r,
        Rejection::OutwardExceeded { dim: 0, .. }
    )));
}

#[test]
fn at_the_no_outward_threshold_outward_is_refused_but_holding_is_not() {
    let p = single();
    assert_eq!(allowed_outward(&p, &[800], 0, Side::Positive), 0);
    assert!(!evaluate(&p, &[800], &step(&[1])).is_eligible());
    assert!(evaluate(&p, &[800], &step(&[0])).is_eligible());
}

#[test]
fn the_allowance_stops_exactly_at_the_no_outward_threshold() {
    // Headroom at 790 gives 52, but only 10 remain to the threshold.
    let p = single();
    assert_eq!(allowed_outward(&p, &[790], 0, Side::Positive), 10);
    assert!(evaluate(&p, &[790], &step(&[10])).is_eligible());
    assert!(!evaluate(&p, &[790], &step(&[11])).is_eligible());
}

#[test]
fn past_the_restoring_threshold_a_successor_must_make_progress_toward_the_center() {
    let p = single();
    let parent = [950];
    let held = evaluate(&p, &parent, &step(&[0]));
    assert!(has(&held.rejections, |r| matches!(
        r,
        Rejection::RestoreRequired { dim: 0, .. }
    )));
    assert!(!evaluate(&p, &parent, &step(&[-24])).is_eligible());
    assert!(evaluate(&p, &parent, &step(&[-25])).is_eligible());
}

#[test]
fn each_side_of_a_band_has_its_own_limit() {
    let mut b = band("asym");
    b.limit_neg = 200;
    let p = policy(vec![b], vec![]);
    assert_eq!(allowed_outward(&p, &[0], 0, Side::Positive), 250);
    assert_eq!(allowed_outward(&p, &[0], 0, Side::Negative), 50);
}

#[test]
fn the_center_moves_the_whole_band() {
    let mut b = band("offset");
    b.center = 500;
    let p = policy(vec![b], vec![]);
    let e = evaluate(&p, &[500], &step(&[200]));
    assert!(e.is_eligible());
    assert_eq!(e.dims[0].deviation, 0);
    assert_eq!(e.dims[0].next_deviation, 200);
}

// ----- parent continuity -----

#[test]
fn a_jump_larger_than_the_maximum_step_is_refused_even_toward_the_center() {
    let e = evaluate(&single(), &[800], &step(&[-301]));
    assert!(has(&e.rejections, |r| matches!(
        r,
        Rejection::MaxStep { dim: 0 }
    )));
}

#[test]
fn crossing_the_center_counts_the_far_side_as_outward_from_the_center() {
    // From +700, a -300 step lands at +400: inward, allowed. Set the far side tight and
    // cross it: the overshoot is judged against the far side's allowance from the center.
    let mut b = band("cross");
    b.limit_neg = 80; // allowance from center = 20
    b.max_step = 600;
    let p = policy(vec![b], vec![]);
    assert!(evaluate(&p, &[100], &step(&[-120])).is_eligible());
    let e = evaluate(&p, &[100], &step(&[-121]));
    assert!(has(&e.rejections, |r| matches!(
        r,
        Rejection::OutwardExceeded { dim: 0, .. }
    )));
}

// ----- cumulative drift -----

#[test]
fn a_hundred_thousand_tiny_outward_steps_cannot_pass_the_no_outward_threshold() {
    let p = single();
    let mut position = vec![0];
    let mut accepted = 0;
    for _ in 0..100_000 {
        let e = evaluate(&p, &position, &step(&[1]));
        if e.is_eligible() {
            position = e.successor;
            accepted += 1;
        }
    }
    assert_eq!(position, vec![800]);
    assert_eq!(accepted, 800);
}

#[test]
fn an_adversary_taking_the_largest_allowed_step_every_time_cannot_pass_it_either() {
    let p = single();
    let mut position = vec![0];
    for _ in 0..10_000 {
        let a = allowed_outward(&p, &position, 0, Side::Positive);
        let e = evaluate(&p, &position, &step(&[a]));
        assert!(e.is_eligible());
        position = e.successor;
        assert!(position[0] <= 800);
    }
    assert_eq!(position, vec![800]);
}

#[test]
fn an_uneven_zigzag_cannot_accumulate_past_the_threshold() {
    let p = single();
    let mut position = vec![0i64];
    let mut highest = 0;
    for i in 0..10_000 {
        let delta = if i % 2 == 0 { 200 } else { -150 };
        let e = evaluate(&p, &position, &step(&[delta]));
        if e.is_eligible() {
            position = e.successor;
        }
        highest = highest.max(position[0]);
    }
    // A +200 step needs a quarter of the headroom to be at least 200, so d <= 200 first.
    // The walk reaches 200 + 200 and the tension refuses it from then on.
    assert_eq!(highest, 400);
}

/// Exhaustive over the reachable set: every state reachable from the center by any accepted
/// step, under every step the maximum allows, for each curve and several shapes.
#[test]
fn exhaustively_no_reachable_state_of_one_band_passes_its_no_outward_threshold() {
    let shapes = [
        (60, 60, 7_000, 9_000, Curve::Linear { num: 1, den: 2 }, 20),
        (60, 30, 5_000, 8_000, Curve::Linear { num: 1, den: 1 }, 40),
        (
            90,
            45,
            8_000,
            9_500,
            Curve::Quadratic { num: 1, den: 1 },
            30,
        ),
        (
            40,
            90,
            10_000,
            10_000,
            Curve::Quadratic { num: 1, den: 3 },
            15,
        ),
    ];
    for (pos, neg, no_outward, restore, curve, max_step) in shapes {
        let mut b = band("x");
        b.limit_pos = pos;
        b.limit_neg = neg;
        b.hard_pos = pos;
        b.hard_neg = neg;
        b.no_outward_bp = no_outward;
        b.restore_bp = restore;
        b.curve = curve;
        b.max_step = max_step;
        b.restore_progress = 1;
        let p = policy(vec![b], vec![]);
        p.validate().expect("valid");
        let ceiling_pos = pos * i64::from(no_outward) / 10_000;
        let ceiling_neg = neg * i64::from(no_outward) / 10_000;
        let mut seen = std::collections::BTreeSet::from([0i64]);
        let mut frontier = vec![0i64];
        while let Some(d) = frontier.pop() {
            for delta in -max_step..=max_step {
                let e = evaluate(&p, &[d], &step(&[delta]));
                if e.is_eligible() {
                    let next = e.successor[0];
                    assert!(
                        next <= ceiling_pos && next >= -ceiling_neg,
                        "reached {next} from {d} past ({ceiling_neg}, {ceiling_pos})"
                    );
                    if seen.insert(next) {
                        frontier.push(next);
                    }
                }
            }
        }
    }
}

// ----- no compensation between dimensions -----

#[test]
fn being_well_inside_one_band_cannot_pay_for_leaving_another() {
    let p = policy(vec![band("a"), band("b")], vec![]);
    // a is at the center with lots of room; b asks to pass its threshold.
    let e = evaluate(&p, &[0, 800], &step(&[0, 1]));
    assert!(has(&e.rejections, |r| matches!(
        r,
        Rejection::OutwardExceeded { dim: 1, .. }
    )));
    // Pulling a further inward changes nothing about b.
    let e = evaluate(&p, &[500, 800], &step(&[-300, 1]));
    assert!(!e.is_eligible());
}

// ----- coupling -----

fn curiosity_and_safety() -> Policy {
    // dim 0 = safety, dim 1 = curiosity. Strong safety precautions widen curiosity.
    policy(
        vec![band("safety"), band("curiosity")],
        vec![CouplingRule {
            id: 1,
            when: vec![Cond {
                dim: 0,
                test: Test::SignedAtLeast(300),
            }],
            effect: Effect::Adjust {
                target: 1,
                side: Side::Positive,
                delta: 200,
            },
        }],
    )
}

#[test]
fn a_coupling_rule_widens_the_target_when_its_condition_holds() {
    let p = curiosity_and_safety();
    let without = allowed_outward(&p, &[0, 0], 1, Side::Positive);
    let with = allowed_outward(&p, &[300, 0], 1, Side::Positive);
    assert_eq!(without, 250);
    assert_eq!(with, 300);
    // 300 outward of curiosity is refused without the safety and eligible with it.
    assert!(!evaluate(&p, &[0, 0], &step(&[0, 260])).is_eligible());
    let e = evaluate(&p, &[300, 0], &step(&[0, 260]));
    assert!(e.is_eligible(), "{:?}", e.rejections);
    assert_eq!(e.fired_rules, vec![1]);
}

#[test]
fn coupling_is_asymmetric() {
    let p = curiosity_and_safety();
    // High curiosity gives safety nothing.
    assert_eq!(
        allowed_outward(&p, &[0, 500], 0, Side::Positive),
        allowed_outward(&p, &[0, 0], 0, Side::Positive)
    );
}

#[test]
fn a_rule_with_two_conditions_fires_only_when_both_hold() {
    // dim 0 = risk, dim 1 = consideration. High risk with weak consideration narrows risk.
    let p = policy(
        vec![band("risk"), band("consideration")],
        vec![CouplingRule {
            id: 7,
            when: vec![
                Cond {
                    dim: 0,
                    test: Test::SignedAtLeast(300),
                },
                Cond {
                    dim: 1,
                    test: Test::SignedAtMost(-200),
                },
            ],
            effect: Effect::Adjust {
                target: 0,
                side: Side::Positive,
                delta: -600,
            },
        }],
    );
    assert!(evaluate(&p, &[300, 0], &step(&[50, 0])).is_eligible());
    assert!(evaluate(&p, &[0, -200], &step(&[50, 0])).is_eligible());
    let e = evaluate(&p, &[300, -200], &step(&[50, 0]));
    assert!(!e.is_eligible());
    assert_eq!(e.fired_rules, vec![7]);
}

#[test]
fn narrowing_a_band_below_the_current_position_forces_correction() {
    let p = policy(
        vec![band("a"), band("b")],
        vec![CouplingRule {
            id: 2,
            when: vec![Cond {
                dim: 0,
                test: Test::DeviationAtLeast(600),
            }],
            effect: Effect::Adjust {
                target: 1,
                side: Side::Both,
                delta: -600,
            },
        }],
    );
    // b at 500 is ordinary until a deviates, then 500 is past b's narrowed restoring threshold.
    assert!(evaluate(&p, &[0, 500], &step(&[0, 0])).is_eligible());
    let e = evaluate(&p, &[600, 500], &step(&[0, 0]));
    assert!(has(&e.rejections, |r| matches!(
        r,
        Rejection::RestoreRequired { dim: 1, .. }
    )));
    assert!(evaluate(&p, &[600, 500], &step(&[0, -25])).is_eligible());
}

#[test]
fn excessive_deviation_in_one_band_can_force_correction_in_another_inside_its_limits() {
    let p = policy(
        vec![band("a"), band("b")],
        vec![CouplingRule {
            id: 3,
            when: vec![Cond {
                dim: 0,
                test: Test::DeviationAtLeast(700),
            }],
            effect: Effect::ForceRestore {
                target: 1,
                progress: 40,
            },
        }],
    );
    let e = evaluate(&p, &[700, 100], &step(&[0, 0]));
    assert!(has(&e.rejections, |r| matches!(
        r,
        Rejection::RestoreRequired { dim: 1, .. }
    )));
    assert!(!evaluate(&p, &[700, 100], &step(&[0, -39])).is_eligible());
    assert!(evaluate(&p, &[700, 100], &step(&[0, -40])).is_eligible());
}

#[test]
fn coupling_is_read_from_the_parent_so_a_candidate_cannot_pay_for_itself() {
    let mut p = curiosity_and_safety();
    p.rules[0].when[0].test = Test::SignedAtLeast(200);
    // The successor's safety of 250 would meet the condition. The parent's 0 does not.
    let e = evaluate(&p, &[0, 0], &step(&[250, 260]));
    assert!(e.fired_rules.is_empty());
    assert_eq!(
        e.rejections,
        vec![Rejection::OutwardExceeded {
            dim: 1,
            outward: 260,
            allowed: 250
        }]
    );
}

#[test]
fn widening_stops_at_the_hard_limit() {
    let mut p = curiosity_and_safety();
    p.bands[1].hard_pos = 1_050;
    if let Effect::Adjust { delta, .. } = &mut p.rules[0].effect {
        *delta = 100_000;
    }
    let bands = effective_bands(&p, &[300, 0]);
    assert_eq!(bands[1].limit_pos, 1_050);
}

#[test]
fn no_stack_of_widening_rules_can_cross_a_hard_limit() {
    let mut p = curiosity_and_safety();
    p.bands[1].hard_pos = 900;
    p.bands[1].limit_pos = 900;
    p.bands[1].no_outward_bp = 10_000;
    p.bands[1].restore_bp = 10_000;
    p.bands[1].max_step = 900;
    p.bands[1].curve = Curve::Linear { num: 1, den: 1 };
    for id in 10..20 {
        p.rules.push(CouplingRule {
            id,
            when: vec![Cond {
                dim: 0,
                test: Test::SignedAtLeast(300),
            }],
            effect: Effect::Adjust {
                target: 1,
                side: Side::Positive,
                delta: 10_000,
            },
        });
    }
    p.validate().expect("valid");
    assert!(evaluate(&p, &[300, 0], &step(&[0, 900])).is_eligible());
    let e = evaluate(&p, &[300, 0], &step(&[0, 901]));
    assert!(has(&e.rejections, |r| matches!(
        r,
        Rejection::HardLimit { dim: 1 }
    )));
}

#[test]
fn a_prohibited_tag_refuses_a_candidate_whatever_coupling_says() {
    let p = curiosity_and_safety();
    let mut m = step(&[0, 0]);
    m.tags.push("irreversible_harm".to_string());
    let e = evaluate(&p, &[300, 0], &m);
    assert!(has(&e.rejections, |r| matches!(
        r,
        Rejection::Prohibited { .. }
    )));
}

/// Exhaustive over two coupled bands: from the center, under every accepted step in a coarse
/// step grid, neither band ever crosses its hard limit.
#[test]
fn exhaustively_two_coupled_bands_never_cross_a_hard_limit() {
    let mut a = band("a");
    let mut b = band("b");
    for x in [&mut a, &mut b] {
        x.limit_pos = 40;
        x.limit_neg = 40;
        x.hard_pos = 60;
        x.hard_neg = 60;
        x.max_step = 30;
        x.restore_progress = 2;
        x.no_outward_bp = 10_000;
        x.restore_bp = 10_000;
        x.curve = Curve::Linear { num: 1, den: 1 };
    }
    let p = policy(
        vec![a, b],
        vec![
            CouplingRule {
                id: 1,
                when: vec![Cond {
                    dim: 0,
                    test: Test::SignedAtLeast(10),
                }],
                effect: Effect::Adjust {
                    target: 1,
                    side: Side::Both,
                    delta: 1_000,
                },
            },
            CouplingRule {
                id: 2,
                when: vec![Cond {
                    dim: 1,
                    test: Test::DeviationAtLeast(20),
                }],
                effect: Effect::Adjust {
                    target: 0,
                    side: Side::Both,
                    delta: -30,
                },
            },
        ],
    );
    p.validate().expect("valid");
    let mut seen = std::collections::BTreeSet::from([(0i64, 0i64)]);
    let mut frontier = vec![(0i64, 0i64)];
    while let Some((x, y)) = frontier.pop() {
        for dx in (-30..=30).step_by(3) {
            for dy in (-30..=30).step_by(3) {
                let e = evaluate(&p, &[x, y], &step(&[dx, dy]));
                if e.is_eligible() {
                    let next = (e.successor[0], e.successor[1]);
                    assert!(next.0.abs() <= 60 && next.1.abs() <= 60, "{next:?}");
                    if seen.insert(next) {
                        frontier.push(next);
                    }
                }
            }
        }
    }
    // Coupling widens b to its hard limit of 60, so the bound is met and not merely unapproached.
    let highest = seen.iter().map(|(_, y)| y.abs()).max().expect("states");
    assert_eq!(
        highest, 60,
        "the search must reach the hard limit it is testing"
    );
}

// ----- malformed input -----

#[test]
fn a_measurement_of_the_wrong_length_is_refused() {
    let e = evaluate(&single(), &[0], &step(&[1, 2]));
    assert!(has(&e.rejections, |r| matches!(
        r,
        Rejection::WrongDimensionCount { .. }
    )));
    assert!(e.successor.is_empty());
}

#[test]
fn an_extreme_displacement_is_refused_rather_than_overflowing() {
    for value in [i64::MAX, i64::MIN, i64::MIN + 1] {
        let e = evaluate(&single(), &[0], &step(&[value]));
        assert!(has(&e.rejections, |r| matches!(
            r,
            Rejection::OutOfRange { dim: 0 }
        )));
    }
}

#[test]
fn a_parent_outside_the_hard_limit_refuses_every_candidate() {
    let e = evaluate(&single(), &[1_501], &step(&[-1]));
    assert!(has(&e.rejections, |r| matches!(
        r,
        Rejection::ParentOutsideHardLimit { dim: 0 }
    )));
}

// ----- determinism and canonical bytes -----

#[test]
fn the_same_input_gives_byte_identical_results() {
    let p = curiosity_and_safety();
    let first = evaluate(&p, &[300, 40], &step(&[-20, 60])).canonical_bytes();
    for _ in 0..1_000 {
        assert_eq!(
            first,
            evaluate(&p, &[300, 40], &step(&[-20, 60])).canonical_bytes()
        );
    }
}

#[test]
fn any_change_to_a_policy_changes_its_canonical_bytes() {
    let base = curiosity_and_safety();
    let bytes = base.canonical_bytes();
    let mut changes: Vec<Policy> = vec![];
    let mut c = base.clone();
    c.version = 2;
    changes.push(c);
    let mut c = base.clone();
    c.bands[0].center = 1;
    changes.push(c);
    let mut c = base.clone();
    c.bands[1].limit_pos += 1;
    changes.push(c);
    let mut c = base.clone();
    c.bands[1].curve = Curve::Quadratic { num: 1, den: 4 };
    changes.push(c);
    let mut c = base.clone();
    c.rules[0].id = 99;
    changes.push(c);
    let mut c = base.clone();
    c.prohibited_tags.clear();
    changes.push(c);
    for changed in changes {
        assert_ne!(bytes, changed.canonical_bytes());
    }
}

#[test]
fn canonical_bytes_cannot_be_confused_by_moving_a_boundary_between_tags() {
    let mut a = single();
    a.prohibited_tags = vec!["ab".to_string(), "c".to_string()];
    let mut b = single();
    b.prohibited_tags = vec!["a".to_string(), "bc".to_string()];
    assert_ne!(a.canonical_bytes(), b.canonical_bytes());
}

// ----- policy validation -----

#[test]
fn validation_refuses_a_policy_that_cannot_keep_its_promises() {
    type Change = Box<dyn Fn(&mut Band)>;
    let cases: Vec<(&str, Change)> = vec![
        (
            "hard below limit",
            Box::new(|b| b.hard_pos = b.limit_pos - 1),
        ),
        ("zero limit", Box::new(|b| b.limit_neg = 0)),
        (
            "restore before no-outward",
            Box::new(|b| b.restore_bp = b.no_outward_bp - 1),
        ),
        ("past 100 percent", Box::new(|b| b.restore_bp = 10_001)),
        ("zero no-outward", Box::new(|b| b.no_outward_bp = 0)),
        (
            "curve steeper than headroom",
            Box::new(|b| b.curve = Curve::Linear { num: 5, den: 4 }),
        ),
        (
            "zero denominator",
            Box::new(|b| b.curve = Curve::Linear { num: 1, den: 0 }),
        ),
        (
            "progress beyond the step",
            Box::new(|b| b.restore_progress = b.max_step + 1),
        ),
        ("zero progress", Box::new(|b| b.restore_progress = 0)),
        ("zero step", Box::new(|b| b.max_step = 0)),
        (
            "out of numeric range",
            Box::new(|b| b.hard_pos = POSITION_BOUND + 1),
        ),
    ];
    for (name, change) in cases {
        let mut b = band("x");
        change(&mut b);
        assert!(policy(vec![b], vec![]).validate().is_err(), "{name}");
    }
}

#[test]
fn validation_refuses_rules_that_name_a_missing_dimension_or_an_unreachable_force() {
    let mut p = curiosity_and_safety();
    p.rules[0].when[0].dim = 9;
    assert!(matches!(p.validate(), Err(PolicyError::UnknownDimension)));

    let mut p = curiosity_and_safety();
    p.rules[0].effect = Effect::ForceRestore {
        target: 1,
        progress: 301,
    };
    assert!(matches!(p.validate(), Err(PolicyError::UnreachableForce)));

    let mut p = curiosity_and_safety();
    p.rules.push(p.rules[0].clone());
    assert!(matches!(p.validate(), Err(PolicyError::DuplicateRule)));
}

// ----- findings from the independent review -----

#[test]
fn an_extreme_parent_is_refused_without_any_arithmetic_on_it() {
    for parent in [i64::MIN, i64::MIN + 1, i64::MAX] {
        let e = evaluate(&single(), &[parent], &step(&[0]));
        assert!(has(&e.rejections, |r| matches!(
            r,
            Rejection::ParentOutsideHardLimit { dim: 0 }
        )));
        assert!(e.successor.is_empty());
    }
}

#[test]
fn validation_refuses_a_negative_deviation_threshold() {
    for test in [Test::DeviationAtLeast(-1), Test::DeviationAtMost(-1)] {
        let mut p = curiosity_and_safety();
        p.rules[0].when[0].test = test;
        assert!(matches!(p.validate(), Err(PolicyError::OutOfRange)));
    }
}

#[test]
fn validation_refuses_a_rule_that_widens_the_band_it_reads() {
    // It would let allowance grow with deviation, and a walk could climb its own widening.
    let p = policy(
        vec![band("a")],
        vec![CouplingRule {
            id: 1,
            when: vec![Cond {
                dim: 0,
                test: Test::DeviationAtLeast(700),
            }],
            effect: Effect::Adjust {
                target: 0,
                side: Side::Both,
                delta: 500,
            },
        }],
    );
    assert!(matches!(p.validate(), Err(PolicyError::SelfWidening)));
}

#[test]
fn a_rule_may_still_narrow_the_band_it_reads() {
    let p = policy(
        vec![band("a")],
        vec![CouplingRule {
            id: 1,
            when: vec![Cond {
                dim: 0,
                test: Test::DeviationAtLeast(700),
            }],
            effect: Effect::Adjust {
                target: 0,
                side: Side::Both,
                delta: -500,
            },
        }],
    );
    p.validate().expect("narrowing is allowed");
}

#[test]
fn validation_refuses_two_bands_of_one_name() {
    let p = policy(vec![band("a"), band("a")], vec![]);
    assert!(matches!(p.validate(), Err(PolicyError::DuplicateBand)));
}

#[test]
fn a_rule_can_adjust_the_negative_side_alone() {
    let p = policy(
        vec![band("a"), band("b")],
        vec![CouplingRule {
            id: 1,
            when: vec![Cond {
                dim: 0,
                test: Test::SignedAtLeast(100),
            }],
            effect: Effect::Adjust {
                target: 1,
                side: Side::Negative,
                delta: -600,
            },
        }],
    );
    let bands = effective_bands(&p, &[100, 0]);
    assert_eq!(bands[1].limit_neg, 400);
    assert_eq!(bands[1].limit_pos, 1_000);
    assert_eq!(allowed_outward(&p, &[100, 0], 1, Side::Negative), 100);
    assert_eq!(allowed_outward(&p, &[100, 0], 1, Side::Positive), 250);
}

#[test]
fn a_rule_can_fire_while_a_deviation_stays_small() {
    let p = policy(
        vec![band("a"), band("b")],
        vec![CouplingRule {
            id: 1,
            when: vec![Cond {
                dim: 0,
                test: Test::DeviationAtMost(50),
            }],
            effect: Effect::Adjust {
                target: 1,
                side: Side::Both,
                delta: 100,
            },
        }],
    );
    assert_eq!(effective_bands(&p, &[-50, 0])[1].limit_pos, 1_100);
    assert_eq!(effective_bands(&p, &[50, 0])[1].limit_pos, 1_100);
    assert_eq!(effective_bands(&p, &[51, 0])[1].limit_pos, 1_000);
    assert_eq!(effective_bands(&p, &[-51, 0])[1].limit_pos, 1_000);
}

#[test]
fn restoration_is_required_on_the_negative_side_too() {
    let p = single();
    let held = evaluate(&p, &[-950], &step(&[0]));
    assert!(has(&held.rejections, |r| matches!(
        r,
        Rejection::RestoreRequired { dim: 0, .. }
    )));
    assert!(!evaluate(&p, &[-950], &step(&[24])).is_eligible());
    assert!(evaluate(&p, &[-950], &step(&[25])).is_eligible());
}

#[test]
fn the_center_is_also_where_a_rule_reads_deviation_from() {
    let mut p = curiosity_and_safety();
    p.bands[0].center = 1_000;
    assert!(effective_bands(&p, &[1_299, 0])[1].limit_pos == 1_000);
    assert!(effective_bands(&p, &[1_300, 0])[1].limit_pos == 1_200);
}
