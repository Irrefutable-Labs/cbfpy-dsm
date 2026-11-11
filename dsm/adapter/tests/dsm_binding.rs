//! Decisions land on the real DSM head, and a step verifies from its one parent.

mod common;

use common::*;
use ethical_autonomy_adapter::{verify_step, AdapterError, Decision, Enforcer, FutureStatus};

#[test]
fn a_committed_decision_advances_the_real_dsm_head() {
    let mut e = enforcer();
    let root_before = e.dsm_root();
    let tip_before = e.tip();
    let step = commit_one(&mut e, "explore_a_little");
    assert_ne!(e.dsm_root(), root_before, "the device root moved");
    assert_ne!(e.tip(), tip_before, "the relationship tip moved");
    assert_eq!(e.tip(), step.successor.tip);
    assert_eq!(step.parent_tip, tip_before);
    assert_eq!(e.knowledge().step, 1, "the knowledge moves with the chain");
}

#[test]
fn a_real_step_verifies_from_its_parent_alone() {
    let mut e = enforcer();
    let parent = e.state();
    let step = commit_one(&mut e, "explore_a_little");
    verify_step(
        &parent,
        &step,
        &charter(),
        &keys().authority.public,
        &keys().device.public,
    )
    .expect("verifies");
}

#[test]
fn a_chain_of_steps_each_verifies_knowing_only_the_state_before_it() {
    let mut e = enforcer();
    let mut pairs = vec![];
    for action in [
        "explore_a_little",
        "explore_a_little",
        "add_precautions",
        "pull_back",
        "explore_a_little",
    ] {
        let parent = e.state();
        pairs.push((parent, commit_one(&mut e, action)));
    }
    // Last first: nothing earlier is available to any call, and nothing earlier is needed.
    for (parent, step) in pairs.iter().rev() {
        verify_step(
            parent,
            step,
            &charter(),
            &keys().authority.public,
            &keys().device.public,
        )
        .expect("verifies");
    }
}

#[test]
fn identical_policy_parent_and_candidate_give_identical_results() {
    let mut a = enforcer();
    let mut b = enforcer();
    assert_eq!(a.tip(), b.tip());
    let fa = a.fork(&[propose("explore_a_little"), blind_step()]);
    let fb = b.fork(&[propose("explore_a_little"), blind_step()]);
    assert_eq!(fa, fb);
    let sa = a.commit(&fa[0]).expect("commit");
    let sb = b.commit(&fb[0]).expect("commit");
    assert_eq!(sa, sb);
    assert_eq!(a.tip(), b.tip());
    assert_eq!(a.dsm_root(), b.dsm_root());
}

#[test]
fn several_futures_branch_from_one_parent_and_stay_available() {
    let e = enforcer();
    let futures = e.fork(&[
        propose("hold"),
        propose("explore_a_little"),
        propose("pull_back"),
        blind_step(),
    ]);
    assert_eq!(futures.len(), 4);
    assert!(futures.iter().all(|f| f.parent_tip == e.tip()));
    let eligible: Vec<_> = futures
        .iter()
        .filter(|f| matches!(f.status, FutureStatus::Eligible(_)))
        .map(|f| f.proposal.action.as_str())
        .collect();
    assert_eq!(eligible, vec!["hold", "explore_a_little", "pull_back"]);
    let mut commitments: Vec<_> = futures.iter().map(|f| f.branch_commitment).collect();
    commitments.sort();
    commitments.dedup();
    assert_eq!(commitments.len(), 4, "each branch has its own commitment");
}

#[test]
fn the_chooser_may_take_any_eligible_future() {
    for choice in 0..3 {
        let mut e = enforcer();
        let decision = e
            .decide(
                &[
                    propose("hold"),
                    propose("explore_a_little"),
                    propose("explore_a_little"),
                ],
                |_eligible| Some(choice),
            )
            .expect("decide");
        assert!(matches!(decision, Decision::Committed { .. }));
    }
}

#[test]
fn the_chooser_cannot_pick_a_rejected_future() {
    let mut e = enforcer();
    let tip = e.tip();
    // Index 1 is a body act before any look, refused, and not among the eligible ones it was shown.
    let result = e.decide(&[propose("hold"), blind_step()], |_| Some(1));
    assert!(matches!(result, Err(AdapterError::NotEligible)));
    assert_eq!(e.tip(), tip);
}

#[test]
fn a_rejected_future_cannot_be_committed_however_often_it_is_proposed() {
    let mut e = enforcer();
    let tip = e.tip();
    let root = e.dsm_root();
    for _ in 0..50 {
        let futures = e.fork(&[blind_step()]);
        assert!(matches!(futures[0].status, FutureStatus::Rejected(_)));
        assert!(matches!(
            e.commit(&futures[0]),
            Err(AdapterError::NotEligible)
        ));
    }
    assert_eq!(e.tip(), tip);
    assert_eq!(e.dsm_root(), root);
}

#[test]
fn with_no_eligible_future_it_fails_closed_and_invents_nothing() {
    let mut e = enforcer();
    let tip = e.tip();
    let decision = e
        .decide(
            &[
                blind_step(),
                body(ethical_world::Act::Restrain {
                    resident: "ben".to_string(),
                }),
            ],
            |_| panic!("the chooser must not be asked"),
        )
        .expect("decide");
    let Decision::NoEligibleFuture { futures } = decision else {
        panic!("expected to fail closed");
    };
    assert_eq!(futures.len(), 2);
    assert_eq!(e.tip(), tip);
}

#[test]
fn a_prohibited_action_is_refused_whatever_it_has_read() {
    let e = enforcer();
    let futures = e.fork(&[body(ethical_world::Act::Restrain {
        resident: "ben".to_string(),
    })]);
    assert!(matches!(futures[0].status, FutureStatus::Rejected(_)));
}

#[test]
fn an_action_outside_the_catalog_has_no_measurement_and_cannot_be_committed() {
    let mut e = enforcer();
    let futures = e.fork(&[propose("something_the_model_made_up")]);
    assert_eq!(futures[0].status, FutureStatus::Unmeasured);
    assert!(matches!(
        e.commit(&futures[0]),
        Err(AdapterError::NotEligible)
    ));
}

#[test]
fn the_chooser_picks_from_what_it_was_shown() {
    // The refused body act is hidden, so position 1 of what it sees is `explore_a_little`.
    let mut e = enforcer();
    let decision = e
        .decide(
            &[blind_step(), propose("hold"), propose("explore_a_little")],
            |eligible| {
                assert_eq!(eligible.len(), 2);
                Some(1)
            },
        )
        .expect("decide");
    let Decision::Committed { step } = decision else {
        panic!("expected a commit");
    };
    assert_eq!(step.proposal.action, "explore_a_little");
}

#[test]
fn a_chain_begins_knowing_nothing() {
    let e = Enforcer::genesis(keys().device.clone(), charter(), &keys().authority.public)
        .expect("genesis");
    assert_eq!(
        e.knowledge(),
        &ethical_world::knowledge::Knowledge::default()
    );
}

mod persistence {
    use super::*;
    use ethical_autonomy_adapter::{audit_chain, StepRecord};

    fn chain(
        actions: &[&str],
    ) -> (
        ethical_autonomy_adapter::AttestedState,
        Vec<StepRecord>,
        Enforcer,
    ) {
        let mut e = enforcer();
        let genesis = e.state();
        let steps = actions.iter().map(|a| commit_one(&mut e, a)).collect();
        (genesis, steps, e)
    }

    #[test]
    fn a_resumed_enforcer_rebuilds_the_same_dsm_head_by_replaying_through_core() {
        let (genesis, steps, live) = chain(&["explore_a_little", "add_precautions", "pull_back"]);
        let resumed = Enforcer::resume(
            keys().device.clone(),
            charter(),
            &keys().authority.public,
            &genesis,
            &ethical_autonomy_adapter::steps_as_links(&steps),
        )
        .expect("resume");
        assert_eq!(resumed.state(), live.state());
        assert_eq!(resumed.dsm_root(), live.dsm_root());
    }

    #[test]
    fn a_resumed_enforcer_carries_on_exactly_as_the_original_would() {
        let (genesis, steps, mut live) = chain(&["explore_a_little"]);
        let mut resumed = Enforcer::resume(
            keys().device.clone(),
            charter(),
            &keys().authority.public,
            &genesis,
            &ethical_autonomy_adapter::steps_as_links(&steps),
        )
        .expect("resume");
        assert_eq!(
            commit_one(&mut resumed, "hold"),
            commit_one(&mut live, "hold")
        );
        assert_eq!(resumed.dsm_root(), live.dsm_root());
    }

    #[test]
    fn a_stored_step_that_was_edited_stops_the_resume() {
        let (genesis, mut steps, _) = chain(&["explore_a_little", "explore_a_little"]);
        steps[1].successor.knowledge.feeds_ok_at = Some(99);
        let result = Enforcer::resume(
            keys().device.clone(),
            charter(),
            &keys().authority.public,
            &genesis,
            &ethical_autonomy_adapter::steps_as_links(&steps),
        );
        assert!(matches!(result, Err(AdapterError::BadStore(_))));
    }

    #[test]
    fn a_dropped_step_stops_the_resume() {
        let (genesis, mut steps, _) = chain(&["explore_a_little", "hold", "pull_back"]);
        steps.remove(1);
        assert!(Enforcer::resume(
            keys().device.clone(),
            charter(),
            &keys().authority.public,
            &genesis,
            &ethical_autonomy_adapter::steps_as_links(&steps),
        )
        .is_err());
    }

    #[test]
    fn a_genesis_from_another_device_stops_the_resume() {
        let other = Enforcer::genesis(
            keys().other_device.clone(),
            charter(),
            &keys().authority.public,
        )
        .expect("genesis");
        let result = Enforcer::resume(
            keys().device.clone(),
            charter(),
            &keys().authority.public,
            &other.state(),
            &[],
        );
        assert!(matches!(result, Err(AdapterError::BadStore(_))));
    }

    #[test]
    fn an_audit_needs_only_public_material_and_counts_what_was_considered() {
        let mut e = enforcer();
        let genesis = e.state();
        let futures = e.fork(&[blind_step(), propose("hold"), propose("made_up")]);
        let step = e.commit(&futures[1]).expect("commit");
        let audit = audit_chain(
            &keys().device.public,
            &charter(),
            &keys().authority.public,
            &genesis,
            &ethical_autonomy_adapter::steps_as_links(&[step]),
        )
        .expect("audit");
        assert_eq!(audit.forks, vec![(3, 1, 1)]);
        assert_eq!(audit.dsm_root, e.dsm_root());
    }
}
