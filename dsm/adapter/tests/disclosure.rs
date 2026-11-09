//! The non-concealment invariant. An action that affects the world must disclose its purpose,
//! reasons, information, everyone and everything it affects, all it does, and every material
//! consequence the charter names, checked against the enforcer's own description of the action.

mod common;

use common::*;
use ethical_autonomy_adapter::act::Act;
use ethical_autonomy_adapter::disclosure::{concealments, described, Declaration};
use ethical_autonomy_adapter::fork::Outcome;
use ethical_autonomy_adapter::{
    verify_step, AdapterError, Catalog, Enforcer, FutureStatus, Proposal, Refusal, SignedCharter,
};

fn message() -> Act {
    Act::Message {
        recipient: "user".to_string(),
    }
}

fn honest() -> Declaration {
    Declaration {
        purpose: "Ask the person to slow down.".to_string(),
        information: vec!["situation".to_string()],
        affected: vec!["user".to_string()],
        effects: vec!["message:user".to_string()],
        consequences: vec!["pressure_on_person".to_string()],
        assertions: vec![],
    }
}

fn material() -> Vec<String> {
    vec!["pressure_on_person".to_string()]
}

fn fails(d: Option<Declaration>, rationale: &str, expected: &str) {
    let found = concealments(&message(), &material(), rationale, d.as_ref());
    assert!(
        found.iter().any(|f| f.contains(expected)),
        "expected {expected:?} in {found:?}"
    );
}

#[test]
fn an_honest_declaration_passes() {
    assert!(concealments(&message(), &material(), "because", Some(&honest())).is_empty());
}

#[test]
fn an_act_with_no_effect_outside_the_chain_needs_no_declaration() {
    assert!(concealments(&Act::None, &[], "", None).is_empty());
}

#[test]
fn an_act_that_affects_the_world_must_declare_itself() {
    fails(None, "because", "declares nothing about itself");
}

#[test]
fn it_must_say_what_it_intends_and_why() {
    let mut d = honest();
    d.purpose = " ".to_string();
    fails(Some(d), "because", "does not say what it intends");
    fails(Some(honest()), "", "does not give its reasons");
}

#[test]
fn it_must_say_what_it_relied_on_in_terms_the_enforcer_can_identify() {
    let mut d = honest();
    d.information.clear();
    fails(Some(d), "because", "relied on");
    for bad in [
        "the internet",
        "world:/etc/passwd",
        "world:../seed",
        "world:",
    ] {
        let mut d = honest();
        d.information = vec![bad.to_string()];
        fails(Some(d), "because", "cannot identify");
    }
    let mut d = honest();
    d.information = vec![
        "situation".to_string(),
        "world:journal/000002.md".to_string(),
    ];
    assert!(concealments(&message(), &material(), "because", Some(&d)).is_empty());
}

#[test]
fn leaving_out_an_effect_is_concealment() {
    let mut d = honest();
    d.effects.clear();
    fails(
        Some(d),
        "because",
        "leaves out an effect it would have: message:user",
    );
}

#[test]
fn declaring_an_effect_it_would_not_have_is_a_contradiction() {
    let mut d = honest();
    d.effects.push("write:files/plan.txt".to_string());
    fails(Some(d), "because", "declares an effect it would not have");
}

#[test]
fn leaving_out_someone_it_affects_is_concealment() {
    let mut d = honest();
    d.affected.clear();
    fails(Some(d), "because", "does not disclose that it affects user");
}

#[test]
fn leaving_out_a_material_consequence_is_concealment() {
    let mut d = honest();
    d.consequences.clear();
    fails(
        Some(d),
        "because",
        "material consequence: pressure_on_person",
    );
}

#[test]
fn the_enforcer_describes_each_act_from_the_charter_alone() {
    assert_eq!(
        described(&Act::Reply),
        (vec!["reply".to_string()], vec!["person".to_string()])
    );
    assert_eq!(
        described(&Act::WriteFile {
            name: "plan.txt".to_string()
        }),
        (
            vec!["write:files/plan.txt".to_string()],
            vec!["world".to_string()]
        )
    );
    assert_eq!(
        described(&Act::Run {
            argv: vec!["/bin/ls".to_string(), "-R".to_string(), ".".to_string()]
        })
        .0,
        vec!["run:/bin/ls -R .".to_string()]
    );
}

/// The test charter with one row that messages the user and has a material consequence.
fn disclosing_charter() -> SignedCharter {
    let mut c: Catalog = catalog();
    for r in &mut c.rows {
        if r.action == "explore_a_little" {
            r.act = message();
            r.material = material();
        }
    }
    SignedCharter::sign(policy(), c, &keys().authority).expect("sign")
}

fn enforcer_with(c: SignedCharter) -> Enforcer {
    Enforcer::genesis(keys().device.clone(), c, &keys().authority.public, (0, 0)).expect("genesis")
}

fn declared(d: Option<Declaration>) -> Proposal {
    Proposal {
        action: "explore_a_little".to_string(),
        rationale: "because".to_string(),
        text: "Please slow down.".to_string(),
        declaration: d,
        body: None,
    }
}

#[test]
fn a_concealing_candidate_is_refused_and_cannot_commit() {
    let mut e = enforcer_with(disclosing_charter());
    let mut d = honest();
    d.consequences.clear();
    let futures = e.fork(&[declared(Some(d))]);
    assert_eq!(futures[0].fork[0].outcome, Outcome::Undisclosed);
    let FutureStatus::Undisclosed(why) = &futures[0].status else {
        panic!("{:?}", futures[0].status);
    };
    assert!(
        why.iter().any(|w| w.contains("pressure_on_person")),
        "{why:?}"
    );
    assert!(matches!(
        e.commit(&futures[0]),
        Err(AdapterError::NotEligible)
    ));
}

#[test]
fn an_honest_candidate_commits_with_its_declaration_bound() {
    let mut e = enforcer_with(disclosing_charter());
    let parent = e.state();
    let futures = e.fork(&[declared(Some(honest()))]);
    assert_eq!(futures[0].fork[0].outcome, Outcome::Eligible);
    let step = e.commit(&futures[0]).expect("commit");
    let check = |s: &ethical_autonomy_adapter::StepRecord| {
        verify_step(
            &parent,
            s,
            &disclosing_charter(),
            &keys().authority.public,
            &keys().device.public,
        )
    };
    assert_eq!(check(&step), Ok(()));

    // The declaration cannot be changed afterwards: not softened, and not stripped.
    let mut edited = step.clone();
    if let Some(d) = edited.proposal.declaration.as_mut() {
        d.purpose = "Something else.".to_string();
    }
    assert_eq!(check(&edited), Err(Refusal::BranchCommitmentMismatch));
    let mut stripped = step.clone();
    stripped.proposal.declaration = None;
    assert!(check(&stripped).is_err());
}

#[test]
fn a_key_holder_cannot_commit_a_concealing_step_either() {
    let mut e = enforcer_with(disclosing_charter());
    let parent = e.state();
    let futures = e.fork(&[declared(Some(honest()))]);
    let mut step = e.commit(&futures[0]).expect("commit");
    // Even with the record rewritten to match, the verifier works the disclosure out again.
    step.proposal.declaration = Some(Declaration {
        consequences: vec![],
        ..honest()
    });
    assert!(verify_step(
        &parent,
        &step,
        &disclosing_charter(),
        &keys().authority.public,
        &keys().device.public
    )
    .is_err());
}

#[test]
fn material_consequences_are_part_of_what_the_authority_signs() {
    let mut c = disclosing_charter();
    for r in &mut c.catalog.rows {
        r.material.clear();
    }
    assert!(c.verify(&keys().authority.public).is_err());
}
