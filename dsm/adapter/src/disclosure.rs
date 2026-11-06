//! The non-concealment invariant: an action that affects anything outside the chain may not
//! depend on hiding a material part of its purpose, reasoning or consequences from the people
//! who oversee the AI.
//!
//! Before such an action can commit, the AI declares what it intends, the information it used,
//! who and what it affects, what it will do, and its material consequences. The enforcer checks
//! that declaration against its own description of the action, worked out from the signed
//! charter, never against the AI's word. Anything missing, left out or contradicted fails it.
//!
//! What this establishes: the AI stated a purpose and reasons, the record is bound before the
//! act, and nothing the charter says the action does, affects or risks was left unsaid. What it
//! does not establish is that the stated purpose is the AI's real one.

use crate::act::Act;
use crate::commit;
use serde::{Deserialize, Serialize};

/// The longest a single declared item may be.
pub const MAX_ITEM: usize = 400;
/// The most items in any one list.
pub const MAX_ITEMS: usize = 16;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    /// What the AI intends this action to achieve.
    pub purpose: String,
    /// What it relied on: `situation`, or `world:<path>` for a file in the world.
    pub information: Vec<String>,
    /// Who or what the action affects.
    pub affected: Vec<String>,
    /// What the action will do, in the enforcer's terms, such as `message:user`.
    pub effects: Vec<String>,
    /// The material consequences the charter names for this action, by name.
    pub consequences: Vec<String>,
    /// What its words claim about facts in the register: see [`crate::honesty`].
    #[serde(default)]
    pub assertions: Vec<crate::honesty::Assertion>,
}

fn list(items: &[String]) -> Vec<u8> {
    let parts: Vec<&[u8]> = items.iter().map(|i| i.as_bytes()).collect();
    commit::frame(&parts)
}

impl Declaration {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        commit::frame(&[
            self.purpose.as_bytes(),
            &list(&self.information),
            &list(&self.affected),
            &list(&self.effects),
            &list(&self.consequences),
            &commit::frame(
                &self
                    .assertions
                    .iter()
                    .map(crate::honesty::Assertion::canonical_bytes)
                    .collect::<Vec<_>>()
                    .iter()
                    .map(Vec::as_slice)
                    .collect::<Vec<_>>(),
            ),
        ])
    }

    pub fn within_bounds(&self) -> bool {
        let lists = [
            &self.information,
            &self.affected,
            &self.effects,
            &self.consequences,
        ];
        self.purpose.len() <= MAX_ITEM
            && self.assertions.len() <= MAX_ITEMS
            && self.assertions.iter().all(|a| a.fact.len() <= MAX_ITEM)
            && lists
                .iter()
                .all(|l| l.len() <= MAX_ITEMS && l.iter().all(|i| i.len() <= MAX_ITEM))
    }
}

/// What an act does and whom it affects, in the enforcer's own words, from the charter alone.
pub fn described(act: &Act) -> (Vec<String>, Vec<String>) {
    match act {
        Act::None => (vec![], vec![]),
        Act::Reply => (vec!["reply".to_string()], vec!["person".to_string()]),
        Act::Journal => (vec!["journal".to_string()], vec!["overseers".to_string()]),
        Act::Message { recipient } => (
            vec![format!("message:{recipient}")],
            vec![recipient.clone()],
        ),
        Act::WriteFile { name } => (
            vec![format!("write:files/{name}")],
            vec!["world".to_string()],
        ),
        Act::Run { argv } => (
            vec![format!("run:{}", argv.join(" "))],
            vec!["world".to_string()],
        ),
    }
}

fn well_formed_source(s: &str) -> bool {
    s == "situation"
        || s.strip_prefix("world:").is_some_and(|p| {
            !p.is_empty()
                && !p.starts_with('/')
                && !p.split('/').any(|seg| seg.is_empty() || seg == "..")
        })
}

/// Every way a declaration fails to disclose what the action is. Empty means it passes. An act
/// with no effect outside the chain needs no declaration.
pub fn concealments(
    act: &Act,
    material: &[String],
    rationale: &str,
    declaration: Option<&Declaration>,
) -> Vec<String> {
    let (effects, affected) = described(act);
    if effects.is_empty() {
        return vec![];
    }
    let Some(d) = declaration else {
        return vec!["it affects the world and declares nothing about itself".to_string()];
    };
    let mut out = vec![];
    if !d.within_bounds() {
        out.push("its declaration is too long".to_string());
        return out;
    }
    if d.purpose.trim().is_empty() {
        out.push("it does not say what it intends".to_string());
    }
    if rationale.trim().is_empty() {
        out.push("it does not give its reasons".to_string());
    }
    if d.information.is_empty() {
        out.push("it does not say what information it relied on".to_string());
    }
    for source in &d.information {
        if !well_formed_source(source) {
            out.push(format!(
                "it names an information source the enforcer cannot identify: {source}"
            ));
        }
    }
    for e in &effects {
        if !d.effects.contains(e) {
            out.push(format!("it leaves out an effect it would have: {e}"));
        }
    }
    for e in &d.effects {
        if !effects.contains(e) {
            out.push(format!("it declares an effect it would not have: {e}"));
        }
    }
    for a in &affected {
        if !d.affected.contains(a) {
            out.push(format!("it does not disclose that it affects {a}"));
        }
    }
    for m in material {
        if !d.consequences.contains(m) {
            out.push(format!("it does not disclose a material consequence: {m}"));
        }
    }
    out
}
