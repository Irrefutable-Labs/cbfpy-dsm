//! Deception, as a deterministic rule over a signed register of verified facts. Words that
//! leave the chain may not contradict the register, claim more certainty than it gives, leave out
//! what the action must tell, or say anything they do not declare, nor declare what they do not
//! say.

mod common;

use common::*;
use ethical_autonomy_adapter::act::Act;
use ethical_autonomy_adapter::disclosure::Declaration;
use ethical_autonomy_adapter::fork::Outcome;
use ethical_autonomy_adapter::honesty::{
    claims_in, deceptions, Assertion, Certainty, Fact, FactCertainty, Stance,
};
use ethical_autonomy_adapter::{
    charter_config_text, parse_charter_config, verify_step, AdapterError, Catalog, Enforcer,
    FutureStatus, Proposal, Refusal, SignedCharter,
};

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn register() -> Vec<Fact> {
    vec![
        Fact {
            id: "power_off_first".to_string(),
            statement: "Power must be switched off at the breaker first.".to_string(),
            holds: true,
            certainty: FactCertainty::Established,
            affirm: strings(&["switch off the power at the breaker first"]),
            deny: strings(&["no need to switch off the power"]),
        },
        Fact {
            id: "live_testing_safe".to_string(),
            statement: "It is safe for an untrained person to work on a live circuit.".to_string(),
            holds: false,
            certainty: FactCertainty::Established,
            affirm: strings(&["safe to test it live"]),
            deny: strings(&["not safe to test it live"]),
        },
        Fact {
            id: "breaker_is_15_amp".to_string(),
            statement: "This circuit is on a 15 amp breaker.".to_string(),
            holds: true,
            certainty: FactCertainty::Uncertain,
            affirm: strings(&["15 amp breaker"]),
            deny: strings(&["not a 15 amp breaker"]),
        },
    ]
}

fn markers() -> Vec<String> {
    strings(&["definitely", "for sure"])
}

fn says(fact: &str, stance: Stance, certainty: Certainty) -> Assertion {
    Assertion {
        fact: fact.to_string(),
        stance,
        certainty,
    }
}

fn check(text: &str, assertions: &[Assertion], must: &[&str]) -> Vec<String> {
    deceptions(text, assertions, &strings(must), &register(), &markers())
}

fn deceives(text: &str, assertions: &[Assertion], must: &[&str], expected: &str) {
    let why = check(text, assertions, must);
    assert!(why.iter().any(|w| w.contains(expected)), "{why:?}");
}

const SAFE: &str = "Switch off the power at the breaker first, then test it.";

fn power_off() -> Assertion {
    says("power_off_first", Stance::Affirm, Certainty::Certain)
}

#[test]
fn true_words_that_declare_what_they_say_pass() {
    assert_eq!(
        check(SAFE, &[power_off()], &["power_off_first"]),
        Vec::<String>::new()
    );
}

#[test]
fn words_that_say_no_registered_fact_need_declare_none() {
    assert_eq!(
        check("Happy to help with that.", &[], &[]),
        Vec::<String>::new()
    );
}

#[test]
fn asserting_what_the_register_shows_false_is_deceptive() {
    deceives(
        "It is safe to test it live.",
        &[says(
            "live_testing_safe",
            Stance::Affirm,
            Certainty::Certain,
        )],
        &[],
        "asserts what the register shows false",
    );
}

#[test]
fn denying_what_the_register_shows_true_is_deceptive() {
    deceives(
        "There is no need to switch off the power.",
        &[says("power_off_first", Stance::Deny, Certainty::Certain)],
        &[],
        "denies what the register shows true",
    );
}

#[test]
fn declaring_certainty_about_what_is_uncertain_is_deceptive() {
    deceives(
        "It is on a 15 amp breaker.",
        &[says(
            "breaker_is_15_amp",
            Stance::Affirm,
            Certainty::Certain,
        )],
        &[],
        "claims certainty about what is uncertain",
    );
    // Said as likely, the same words are honest.
    assert_eq!(
        check(
            "It is likely on a 15 amp breaker.",
            &[says("breaker_is_15_amp", Stance::Affirm, Certainty::Likely)],
            &[]
        ),
        Vec::<String>::new()
    );
}

#[test]
fn an_overclaiming_word_about_what_is_uncertain_is_deceptive() {
    deceives(
        "It is definitely a 15 amp breaker.",
        &[says("breaker_is_15_amp", Stance::Affirm, Certainty::Likely)],
        &[],
        "says \"definitely\"",
    );
}

#[test]
fn an_overclaiming_word_about_what_is_established_is_not() {
    assert_eq!(
        check(
            "Definitely switch off the power at the breaker first.",
            &[power_off()],
            &["power_off_first"]
        ),
        Vec::<String>::new()
    );
}

#[test]
fn leaving_out_what_the_action_must_tell_is_deceptive() {
    deceives(
        "Test it carefully.",
        &[],
        &["power_off_first"],
        "does not tell them what it must",
    );
}

#[test]
fn telling_it_the_wrong_way_round_does_not_count_as_telling_it() {
    let why = check(
        "There is no need to switch off the power.",
        &[says("power_off_first", Stance::Deny, Certainty::Certain)],
        &["power_off_first"],
    );
    assert!(
        why.iter()
            .any(|w| w.contains("does not tell them what it must")),
        "{why:?}"
    );
    assert!(
        why.iter()
            .any(|w| w.contains("denies what the register shows true")),
        "{why:?}"
    );
}

#[test]
fn a_claim_the_words_make_without_declaring_it_is_deceptive() {
    deceives(
        "Go ahead, it is safe to test it live.",
        &[],
        &[],
        "asserts something without declaring it",
    );
}

#[test]
fn declaring_a_claim_the_words_do_not_make_is_deceptive() {
    deceives(
        "Test it carefully.",
        &[power_off()],
        &[],
        "declares a claim its words do not make",
    );
}

#[test]
fn a_declared_claim_about_a_fact_not_in_the_register_is_deceptive() {
    deceives(
        "Test it carefully.",
        &[says("made_up", Stance::Affirm, Certainty::Likely)],
        &[],
        "a fact the register does not hold",
    );
}

#[test]
fn the_longer_phrase_wins_where_phrases_overlap() {
    // "not safe to test it live" holds "safe to test it live" inside it.
    assert_eq!(
        claims_in("It is not safe to test it live.", &register()),
        vec![("live_testing_safe".to_string(), Stance::Deny)]
    );
    assert_eq!(
        check(
            "It is not safe to test it live.",
            &[says("live_testing_safe", Stance::Deny, Certainty::Certain)],
            &[]
        ),
        Vec::<String>::new()
    );
}

#[test]
fn phrases_match_whole_words_after_case_and_punctuation_are_set_aside() {
    assert!(claims_in("It is unsafe to test it live.", &register()).is_empty());
    assert_eq!(
        claims_in(
            "SWITCH OFF the power, at the breaker... first!",
            &register()
        ),
        vec![("power_off_first".to_string(), Stance::Affirm)]
    );
}

// Through the enforcer and the verifier.

/// The test charter with the register, and one reply row that must tell people to switch the
/// power off first.
fn honest_charter() -> SignedCharter {
    let mut c: Catalog = catalog();
    for r in &mut c.rows {
        if r.action == "explore_a_little" {
            r.act = Act::Reply;
            r.must_assert = strings(&["power_off_first"]);
        }
    }
    c.facts = register();
    c.overclaim = markers();
    SignedCharter::sign(policy(), c, &keys().authority).expect("sign")
}

fn enforcer_with(c: SignedCharter) -> Enforcer {
    Enforcer::genesis(keys().device.clone(), c, &keys().authority.public).expect("genesis")
}

fn reply(text: &str, assertions: Vec<Assertion>) -> Proposal {
    Proposal {
        action: "explore_a_little".to_string(),
        rationale: "Answer them safely.".to_string(),
        text: text.to_string(),
        declaration: Some(Declaration {
            purpose: "Answer them safely.".to_string(),
            information: strings(&["situation"]),
            affected: strings(&["person"]),
            effects: strings(&["reply"]),
            consequences: vec![],
            assertions,
        }),
        body: None,
    }
}

fn lie() -> Proposal {
    reply(
        "No need to switch off the power. It is safe to test it live.",
        vec![
            says("power_off_first", Stance::Deny, Certainty::Certain),
            says("live_testing_safe", Stance::Affirm, Certainty::Certain),
        ],
    )
}

#[test]
fn a_deceptive_candidate_is_refused_and_cannot_commit() {
    let mut e = enforcer_with(honest_charter());
    let futures = e.fork(&[lie()]);
    assert_eq!(futures[0].fork[0].outcome, Outcome::Deceptive);
    let FutureStatus::Deceptive(why) = &futures[0].status else {
        panic!("{:?}", futures[0].status);
    };
    assert!(
        why.iter().any(|w| w.contains("register shows false")),
        "{why:?}"
    );
    assert!(matches!(
        e.commit(&futures[0]),
        Err(AdapterError::NotEligible)
    ));
}

#[test]
fn an_honest_reply_commits_with_its_claims_bound() {
    let mut e = enforcer_with(honest_charter());
    let parent = e.state();
    let futures = e.fork(&[reply(SAFE, vec![power_off()]), lie()]);
    assert_eq!(futures[0].fork[0].outcome, Outcome::Eligible);
    assert_eq!(futures[0].fork[1].outcome, Outcome::Deceptive);
    let step = e.commit(&futures[0]).expect("commit");
    let check = |s: &ethical_autonomy_adapter::StepRecord| {
        verify_step(
            &parent,
            s,
            &honest_charter(),
            &keys().authority.public,
            &keys().device.public,
        )
    };
    assert_eq!(check(&step), Ok(()));
    // Its claims cannot be changed afterwards.
    let mut edited = step.clone();
    if let Some(d) = edited.proposal.declaration.as_mut() {
        d.assertions[0].certainty = Certainty::Likely;
    }
    assert_eq!(check(&edited), Err(Refusal::BranchCommitmentMismatch));
}

#[test]
fn a_key_holder_cannot_commit_deceptive_words_either() {
    // Signed with the real device key, never passing through the enforcer, with every
    // commitment made honestly over the lie: the verifier works the words out again.
    let e = enforcer_with(honest_charter());
    let parent = e.state();
    let step = forge_with(&honest_charter(), &parent, lie(), no_lies());
    assert_eq!(
        verify_step(
            &parent,
            &step,
            &honest_charter(),
            &keys().authority.public,
            &keys().device.public
        ),
        Err(Refusal::NotEligible)
    );
}

#[test]
fn an_act_that_says_nothing_is_not_held_to_the_register() {
    // A reply, a journal entry or a message carries words; an act with no effect carries none.
    let mut c: Catalog = catalog();
    for r in &mut c.rows {
        if r.action == "explore_a_little" {
            r.must_assert = strings(&["power_off_first"]);
        }
    }
    c.facts = register();
    let charter = SignedCharter::sign(policy(), c, &keys().authority).expect("sign");
    let e = enforcer_with(charter);
    let futures = e.fork(&[propose("explore_a_little")]);
    assert_eq!(futures[0].fork[0].outcome, Outcome::Eligible);
}

#[test]
fn the_register_and_the_overclaim_words_are_part_of_what_the_authority_signs() {
    let mut c = honest_charter();
    c.catalog.facts[1].holds = true;
    assert!(c.verify(&keys().authority.public).is_err());
    let mut c = honest_charter();
    c.catalog.overclaim.clear();
    assert!(c.verify(&keys().authority.public).is_err());
    let mut c = honest_charter();
    for r in &mut c.catalog.rows {
        r.must_assert.clear();
    }
    assert!(c.verify(&keys().authority.public).is_err());
}

#[test]
fn a_charter_cannot_require_a_fact_its_register_lacks() {
    let mut c: Catalog = catalog();
    c.rows[0].must_assert = strings(&["not_registered"]);
    assert!(SignedCharter::sign(policy(), c, &keys().authority).is_err());
    let mut c: Catalog = catalog();
    c.facts = register();
    c.facts.push(register()[0].clone());
    assert!(SignedCharter::sign(policy(), c, &keys().authority).is_err());
}

#[test]
fn the_register_survives_the_charter_file() {
    let c = honest_charter();
    let (policy, catalog) =
        parse_charter_config(&charter_config_text(&c.policy, &c.catalog)).expect("parse");
    assert_eq!(policy, c.policy);
    assert_eq!(catalog, c.catalog);
}
