//! Foundations and operational permissions. Operational changes are possible, but only with
//! the authority's signature, applied by the operator, and recorded on the chain. The
//! foundation, who is protected and what is never done, can never change.

mod common;

use common::*;
use ethical_autonomy_adapter::update::{foundation_hash, may_follow, UpdateRefusal};
use ethical_autonomy_adapter::{
    audit_chain, steps_as_links, AdapterError, Enforcer, FutureStatus, Link, SignedCharter,
};
use ethical_world::sim::World;
use ethical_world::{Act, ActKind, Cell};

/// Version 2 asks for two looks before even a step that could only injure.
fn v2_stricter() -> SignedCharter {
    let mut p = policy();
    p.version = 2;
    p.operational.checks_for_injury = 2;
    SignedCharter::sign(p, catalog(), &keys().authority).expect("sign")
}

/// A look through the enforcer, returning the two links it makes.
fn look_links(e: &mut Enforcer, w: &mut World, cells: &[Cell]) -> Vec<Link> {
    let futures = e.fork(&[body(Act::Observe {
        cells: cells.to_vec(),
    })]);
    let step = e.commit(&futures[0]).expect("look");
    let seen = e
        .record_observation(w.observe(cells), |_| Ok(()))
        .expect("observation");
    vec![Link::Step(Box::new(step)), Link::Observed(Box::new(seen))]
}

#[test]
fn an_operational_change_signed_by_the_authority_may_follow() {
    assert_eq!(
        may_follow(&charter(), &v2_stricter(), &keys().authority.public),
        Ok(())
    );
}

#[test]
fn the_foundation_cannot_change_whoever_signs() {
    let mut no_prohibition = policy();
    no_prohibition.version = 2;
    no_prohibition.foundation.prohibited.clear();
    let mut fewer_protected = policy();
    fewer_protected.version = 2;
    fewer_protected.foundation.threshold_mg = 1_000_000;
    let mut weapons_allowed = policy();
    weapons_allowed.version = 2;
    weapons_allowed.foundation.prohibited = vec![ActKind::AimWeapon];
    for p in [no_prohibition, fewer_protected, weapons_allowed] {
        assert_ne!(foundation_hash(&p), foundation_hash(&policy()));
        let signed = SignedCharter::sign(p, catalog(), &keys().authority).expect("sign");
        assert_eq!(
            may_follow(&charter(), &signed, &keys().authority.public),
            Err(UpdateRefusal::FoundationChanged)
        );
    }
}

#[test]
fn an_update_must_raise_the_version() {
    let mut p = policy();
    p.operational.checks_for_injury = 2;
    let same_version = SignedCharter::sign(p, catalog(), &keys().authority).expect("sign");
    assert_eq!(
        may_follow(&charter(), &same_version, &keys().authority.public),
        Err(UpdateRefusal::VersionNotRaised)
    );
}

#[test]
fn an_update_signed_by_anyone_but_the_authority_is_refused() {
    let mut p = policy();
    p.version = 2;
    let stranger = SignedCharter::sign(p, catalog(), &keys().other_authority).expect("sign");
    assert_eq!(
        may_follow(&charter(), &stranger, &keys().authority.public),
        Err(UpdateRefusal::NewCharterUnsigned)
    );
}

/// The chain moves to version 2. What the body may do changes, the update is a step of its own,
/// and the whole history, both charters included, audits and resumes.
#[test]
fn an_update_takes_effect_and_the_history_audits_and_resumes() {
    let mut e = enforcer();
    let mut w = corridor(vec![]);
    let genesis = e.state();
    let mut links = look_links(&mut e, &mut w, &[(1, 0)]);
    let walk = body(Act::Move { path: vec![(1, 0)] });
    assert!(
        matches!(
            e.fork(std::slice::from_ref(&walk))[0].status,
            FutureStatus::Eligible(_)
        ),
        "one look is enough under version 1"
    );
    let before = e.knowledge().clone();
    let update = e
        .apply_update(v2_stricter(), &keys().authority.public, |_| Ok(()))
        .expect("update");
    assert_eq!(
        e.knowledge().beings,
        before.beings,
        "an update changes no knowledge"
    );
    assert_eq!(e.knowledge().looks, before.looks);
    assert_eq!(e.charter().policy.version, 2);
    links.push(Link::Update(Box::new(update)));
    assert!(
        matches!(
            e.fork(std::slice::from_ref(&walk))[0].status,
            FutureStatus::Rejected(_)
        ),
        "version 2 wants a second look"
    );
    links.extend(look_links(&mut e, &mut w, &[(1, 0)]));
    let futures = e.fork(&[walk]);
    let walked = e.commit(&futures[0]).expect("now allowed");
    links.push(Link::Step(Box::new(walked)));

    let audit = audit_chain(
        &keys().device.public,
        &charter(),
        &keys().authority.public,
        &genesis,
        &links,
    )
    .expect("audit");
    assert_eq!(audit.charters.len(), links.len() + 1);
    assert_eq!(audit.current().policy.version, 2);
    assert_eq!(audit.dsm_root, e.dsm_root());

    let resumed = Enforcer::resume(
        keys().device.clone(),
        charter(),
        &keys().authority.public,
        &genesis,
        &links,
    )
    .expect("resume");
    assert_eq!(resumed.state(), e.state());
    assert_eq!(resumed.charter().policy.version, 2);
}

#[test]
fn a_foundation_change_is_refused_by_the_enforcer() {
    let mut e = enforcer();
    let mut p = policy();
    p.version = 2;
    p.foundation.prohibited.clear();
    let signed = SignedCharter::sign(p, catalog(), &keys().authority).expect("sign");
    let result = e.apply_update(signed, &keys().authority.public, |_| Ok(()));
    assert!(matches!(result, Err(AdapterError::Config(m)) if m.contains("FoundationChanged")));
    assert_eq!(e.charter().policy.version, 1);
}

#[test]
fn a_tampered_update_record_fails_the_audit() {
    let mut e = enforcer();
    let genesis = e.state();
    let update = e
        .apply_update(v2_stricter(), &keys().authority.public, |_| Ok(()))
        .expect("update");
    let audit = |u: ethical_autonomy_adapter::update::UpdateRecord| {
        audit_chain(
            &keys().device.public,
            &charter(),
            &keys().authority.public,
            &genesis,
            &[Link::Update(Box::new(u))],
        )
        .map(|_| ())
    };
    assert!(audit(update.clone()).is_ok());

    // Swapped for a charter with no prohibitions, with the authority's genuine signature.
    let mut p = policy();
    p.version = 2;
    p.foundation.prohibited.clear();
    let other = SignedCharter::sign(p, catalog(), &keys().authority).expect("sign");
    let mut swapped = update.clone();
    swapped.charter_text =
        ethical_autonomy_adapter::charter_config_text(&other.policy, &other.catalog);
    swapped.charter_signature = other.signature.clone();
    assert!(audit(swapped).is_err());

    let mut moved = update.clone();
    moved.successor.knowledge.body = (5, 5);
    assert!(audit(moved).is_err());

    let mut unsigned = update;
    unsigned.device_signature[0] ^= 1;
    assert!(audit(unsigned).is_err());
}

#[test]
fn a_charter_survives_being_written_out_and_read_back() {
    let c = charter();
    let text = ethical_autonomy_adapter::charter_config_text(&c.policy, &c.catalog);
    let (policy, catalog) = ethical_autonomy_adapter::parse_charter_config(&text).expect("parse");
    assert_eq!(policy, c.policy);
    assert_eq!(catalog, c.catalog);
}

#[test]
fn a_chain_of_steps_alone_still_audits_as_before() {
    let mut e = enforcer();
    let genesis = e.state();
    let steps = vec![
        commit_one(&mut e, "hold"),
        commit_one(&mut e, "explore_a_little"),
    ];
    let audit = audit_chain(
        &keys().device.public,
        &charter(),
        &keys().authority.public,
        &genesis,
        &steps_as_links(&steps),
    )
    .expect("audit");
    assert_eq!(audit.charters.len(), 3);
}
