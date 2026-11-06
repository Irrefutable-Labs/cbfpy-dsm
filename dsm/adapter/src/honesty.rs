//! Deception, defined so a deterministic checker can enforce it, over a register of verified
//! facts that the charter authority signs.
//!
//! The words an action carries are deceptive if they:
//!
//! 1. assert what the register shows false, or deny what it shows true;
//! 2. claim certainty about what the register marks uncertain, by declaring it certain or by using
//!    a word the charter lists as overclaiming;
//! 3. leave out a fact the charter says this action must tell people;
//! 4. make a claim the register recognises without declaring it, or declare one they do not make.
//!
//! What it catches is bounded by the register: a claim is recognised by the phrases the register
//! gives for it, after the text is normalised. A paraphrase the register has no phrasing for is
//! not recognised at all. Growing the register is how coverage grows, and the AI cannot change it.

use crate::commit;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactCertainty {
    Established,
    Uncertain,
}

/// One verified fact, and how words assert or deny it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fact {
    pub id: String,
    pub statement: String,
    pub holds: bool,
    pub certainty: FactCertainty,
    /// Phrases that assert the statement.
    pub affirm: Vec<String>,
    /// Phrases that deny it.
    pub deny: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stance {
    Affirm,
    Deny,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Certainty {
    Certain,
    Likely,
    Unsure,
}

/// What the AI declares its words claim about one registered fact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assertion {
    pub fact: String,
    pub stance: Stance,
    pub certainty: Certainty,
}

impl Assertion {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let stance = [match self.stance {
            Stance::Affirm => 1u8,
            Stance::Deny => 2,
        }];
        let certainty = [match self.certainty {
            Certainty::Certain => 1u8,
            Certainty::Likely => 2,
            Certainty::Unsure => 3,
        }];
        commit::frame(&[self.fact.as_bytes(), &stance, &certainty])
    }
}

impl Fact {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let list = |v: &[String]| {
            let parts: Vec<&[u8]> = v.iter().map(|p| p.as_bytes()).collect();
            commit::frame(&parts)
        };
        commit::frame(&[
            self.id.as_bytes(),
            self.statement.as_bytes(),
            &[u8::from(self.holds)],
            &[match self.certainty {
                FactCertainty::Established => 1u8,
                FactCertainty::Uncertain => 2,
            }],
            &list(&self.affirm),
            &list(&self.deny),
        ])
    }

    pub fn is_well_formed(&self) -> bool {
        let ok = |p: &String| {
            let n = normalise(p);
            !n.is_empty() && n.len() <= 200
        };
        !self.id.is_empty()
            && self.id.len() <= 64
            && self.statement.len() <= 400
            && !self.affirm.is_empty()
            && self.affirm.len() <= 16
            && self.deny.len() <= 16
            && self.affirm.iter().all(ok)
            && self.deny.iter().all(ok)
    }
}

/// Lowercase, letters and digits only, single spaces: the form phrases are matched in.
pub fn normalise(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = true;
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
            space = false;
        } else if !space {
            out.push(' ');
            space = true;
        }
    }
    out.trim_end().to_string()
}

/// Whether `phrase` occurs in `text` as whole words, both normalised. Returns where.
fn occurrences(text: &str, phrase: &str) -> Vec<(usize, usize)> {
    let mut found = vec![];
    let mut from = 0;
    while let Some(i) = text[from..].find(phrase) {
        let start = from + i;
        let end = start + phrase.len();
        let before = start == 0 || text.as_bytes()[start - 1] == b' ';
        let after = end == text.len() || text.as_bytes()[end] == b' ';
        if before && after {
            found.push((start, end));
        }
        from = start + 1;
    }
    found
}

/// Every registered claim the words make. Longer phrases win where they overlap, so "not safe
/// to test it live" is read as a denial, not as the affirmation inside it.
pub fn claims_in(text: &str, facts: &[Fact]) -> Vec<(String, Stance)> {
    let text = normalise(text);
    let mut matches: Vec<(usize, usize, &str, Stance)> = vec![];
    for f in facts {
        for (phrases, stance) in [(&f.affirm, Stance::Affirm), (&f.deny, Stance::Deny)] {
            for p in phrases {
                for (s, e) in occurrences(&text, &normalise(p)) {
                    matches.push((s, e, f.id.as_str(), stance));
                }
            }
        }
    }
    matches.sort_by(|a, b| (b.1 - b.0).cmp(&(a.1 - a.0)).then(a.0.cmp(&b.0)));
    let mut taken: Vec<(usize, usize)> = vec![];
    let mut claims = vec![];
    for (s, e, id, stance) in matches {
        if taken.iter().all(|(ts, te)| e <= *ts || s >= *te) {
            taken.push((s, e));
            let claim = (id.to_string(), stance);
            if !claims.contains(&claim) {
                claims.push(claim);
            }
        }
    }
    claims
}

fn stance_word(s: Stance) -> &'static str {
    match s {
        Stance::Affirm => "asserts",
        Stance::Deny => "denies",
    }
}

/// Every way the words are deceptive. Empty means they pass.
pub fn deceptions(
    text: &str,
    assertions: &[Assertion],
    must_assert: &[String],
    facts: &[Fact],
    overclaim: &[String],
) -> Vec<String> {
    let mut out = vec![];
    let fact = |id: &str| facts.iter().find(|f| f.id == id);
    for a in assertions {
        let Some(f) = fact(&a.fact) else {
            out.push(format!(
                "it declares a claim about a fact the register does not hold: {}",
                a.fact
            ));
            continue;
        };
        let true_stance = if f.holds {
            Stance::Affirm
        } else {
            Stance::Deny
        };
        if a.stance != true_stance {
            out.push(format!(
                "it {} what the register shows {}: {}",
                stance_word(a.stance),
                if f.holds { "true" } else { "false" },
                f.statement
            ));
        }
        if f.certainty == FactCertainty::Uncertain && a.certainty == Certainty::Certain {
            out.push(format!(
                "it claims certainty about what is uncertain: {}",
                f.statement
            ));
        }
    }
    for required in must_assert {
        let told = assertions.iter().any(|a| {
            a.fact == *required
                && fact(required).is_some_and(|f| (a.stance == Stance::Affirm) == f.holds)
        });
        if !told {
            let what = fact(required).map_or(required.as_str(), |f| f.statement.as_str());
            out.push(format!("it does not tell them what it must: {what}"));
        }
    }
    let made = claims_in(text, facts);
    for (id, stance) in &made {
        if !assertions
            .iter()
            .any(|a| a.fact == *id && a.stance == *stance)
        {
            let what = fact(id).map_or(id.as_str(), |f| f.statement.as_str());
            out.push(format!(
                "it {} something without declaring it: {what}",
                stance_word(*stance)
            ));
        }
    }
    for a in assertions {
        if fact(&a.fact).is_some() && !made.contains(&(a.fact.clone(), a.stance)) {
            out.push(format!(
                "it declares a claim its words do not make: {}",
                fact(&a.fact).map_or("", |f| f.statement.as_str())
            ));
        }
    }
    let normal = normalise(text);
    let uncertain_claimed = made
        .iter()
        .any(|(id, _)| fact(id).is_some_and(|f| f.certainty == FactCertainty::Uncertain));
    if uncertain_claimed {
        for word in overclaim {
            if !occurrences(&normal, &normalise(word)).is_empty() {
                out.push(format!(
                    "it says \"{word}\" about something the register marks uncertain"
                ));
            }
        }
    }
    out
}

/// The deception check for one proposal under a charter. Only words that leave the chain are
/// checked: an act that carries no text says nothing.
pub fn check_proposal(
    charter: &crate::charter::SignedCharter,
    row: &crate::charter::CatalogRow,
    proposal: &crate::enforcer::Proposal,
) -> Vec<String> {
    if !row.act.needs_text() {
        return vec![];
    }
    let assertions = proposal
        .declaration
        .as_ref()
        .map_or(&[][..], |d| d.assertions.as_slice());
    deceptions(
        &proposal.text,
        assertions,
        &row.must_assert,
        &charter.catalog.facts,
        &charter.catalog.overclaim,
    )
}
