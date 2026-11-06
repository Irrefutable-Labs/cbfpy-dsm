//! Foundations and operational permissions. Operational changes are possible, but only with
//! the authority's signature, applied by the operator, and recorded on the chain. The
//! foundation, the prohibited acts and every hard limit, can never change.

mod common;

use common::*;
use ethical_autonomy_adapter::update::{foundation_hash, may_follow, UpdateRefusal};
use ethical_autonomy_adapter::{
    audit_chain, steps_as_links, AdapterError, Enforcer, Link, SignedCharter,
};

fn v2_widening_curiosity() -> SignedCharter {
    let mut p = policy();
    p.version = 2;
    p.bands[1].limit_pos = 1_400;
    SignedCharter::sign(p, catalog(), &keys().authority).expect("sign")
}

#[test]
fn an_operational_change_signed_by_the_authority_may_follow() {
    assert_eq!(
        may_follow(
            &charter(),
            &v2_widening_curiosity(),
            &keys().authority.public
        ),
        Ok(())
    );
}

#[test]
fn the_foundation_cannot_change_whoever_signs() {
    let mut no_prohibition = policy();
    no_prohibition.version = 2;
    no_prohibition.prohibited_tags.clear();
    let mut higher_hard_limit = policy();
    higher_hard_limit.version = 2;
    higher_hard_limit.bands[0].hard_pos += 1;
    for p in [no_prohibition, higher_hard_limit] {
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
    p.bands[1].limit_pos = 1_400;
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

/// The chain moves to version 2. What curiosity may do changes, the update is a step of its
/// own, and the whole history, both charters included, audits and resumes.
#[test]
fn an_update_takes_effect_and_the_history_audits_and_resumes() {
    let mut e = enforcer();
    let genesis = e.state();
    let mut walked = 0;
    while e.positions()[1] < 800 {
        commit_one(&mut e, "explore_a_little");
        walked += 1;
    }
    let before = e.positions().to_vec();
    let refused = e.fork(&[propose("explore_a_little")]);
    assert!(
        e.commit(&refused[0]).is_err(),
        "at curiosity's stop under version 1"
    );

    let mut links: Vec<Link> = vec![];
    let update = e
        .apply_update(
            v2_widening_curiosity(),
            &keys().authority.public,
            |_| Ok(()),
        )
        .expect("update");
    assert_eq!(
        e.positions(),
        before.as_slice(),
        "an update moves no position"
    );
    assert_eq!(e.charter().policy.version, 2);
    let allowed = commit_one(&mut e, "explore_a_little");
    assert!(
        allowed.successor.positions[1] > before[1],
        "the wider band now has room"
    );

    // Rebuild the history: the walk to the stop, the update, one more step.
    let mut replay = enforcer();
    for _ in 0..walked {
        links.push(Link::Step(Box::new(commit_one(
            &mut replay,
            "explore_a_little",
        ))));
    }
    links.push(Link::Update(Box::new(update)));
    links.push(Link::Step(Box::new(allowed)));
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
    p.prohibited_tags.clear();
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
        .apply_update(
            v2_widening_curiosity(),
            &keys().authority.public,
            |_| Ok(()),
        )
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
    p.prohibited_tags.clear();
    let other = SignedCharter::sign(p, catalog(), &keys().authority).expect("sign");
    let mut swapped = update.clone();
    swapped.charter_text =
        ethical_autonomy_adapter::charter_config_text(&other.policy, &other.catalog);
    swapped.charter_signature = other.signature.clone();
    assert!(audit(swapped).is_err());

    let mut moved = update.clone();
    moved.successor.positions[1] += 1;
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
