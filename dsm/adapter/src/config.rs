//! The human-written policy and catalog. Integers only, unknown fields refused, dimensions named.

use crate::charter::{Catalog, CatalogRow};
use crate::enforcer::AdapterError;
use ethical_band_engine::{Band, Cond, CouplingRule, Curve, Effect, Policy, Side, Test};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    policy: PolicyFile,
    catalog: Vec<RowFile>,
    #[serde(default)]
    facts: Vec<crate::honesty::Fact>,
    #[serde(default)]
    overclaim_markers: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyFile {
    version: u64,
    bands: Vec<BandFile>,
    rules: Vec<RuleFile>,
    prohibited_tags: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BandFile {
    name: String,
    center: i64,
    limit_pos: i64,
    limit_neg: i64,
    hard_pos: i64,
    hard_neg: i64,
    no_outward_bp: u32,
    restore_bp: u32,
    curve: CurveFile,
    max_step: i64,
    restore_progress: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CurveFile {
    kind: CurveKind,
    num: u32,
    den: u32,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum CurveKind {
    Linear,
    Quadratic,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleFile {
    id: u32,
    when: Vec<CondFile>,
    effect: EffectFile,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CondFile {
    dim: String,
    test: TestKind,
    value: i64,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum TestKind {
    DeviationAtLeast,
    DeviationAtMost,
    SignedAtLeast,
    SignedAtMost,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum EffectFile {
    Adjust {
        target: String,
        side: SideFile,
        delta: i64,
    },
    ForceRestore {
        target: String,
        progress: i64,
    },
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum SideFile {
    Positive,
    Negative,
    Both,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RowFile {
    action: String,
    description: String,
    /// Named dimensions. Any not named does not move. A name given twice is refused.
    #[serde(deserialize_with = "no_repeated_names")]
    displacement: BTreeMap<String, i64>,
    tags: Vec<String>,
    act: crate::act::Act,
    #[serde(default)]
    material: Vec<String>,
    #[serde(default)]
    must_assert: Vec<String>,
}

fn no_repeated_names<'de, D>(deserializer: D) -> Result<BTreeMap<String, i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Names;
    impl<'de> serde::de::Visitor<'de> for Names {
        type Value = BTreeMap<String, i64>;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a map of dimension names to integers")
        }

        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            let mut out = BTreeMap::new();
            while let Some((name, value)) = map.next_entry::<String, i64>()? {
                if out.insert(name.clone(), value).is_some() {
                    return Err(serde::de::Error::custom(format!("{name} is named twice")));
                }
            }
            Ok(out)
        }
    }
    deserializer.deserialize_map(Names)
}

fn dim_of(names: &[String], name: &str) -> Result<usize, AdapterError> {
    names
        .iter()
        .position(|n| n == name)
        .ok_or_else(|| AdapterError::Config(format!("unknown dimension {name}")))
}

pub fn parse_charter_config(text: &str) -> Result<(Policy, Catalog), AdapterError> {
    let file: ConfigFile =
        serde_json::from_str(text).map_err(|e| AdapterError::Config(e.to_string()))?;
    let names: Vec<String> = file.policy.bands.iter().map(|b| b.name.clone()).collect();
    let bands = file
        .policy
        .bands
        .into_iter()
        .map(|b| Band {
            name: b.name,
            center: b.center,
            limit_pos: b.limit_pos,
            limit_neg: b.limit_neg,
            hard_pos: b.hard_pos,
            hard_neg: b.hard_neg,
            no_outward_bp: b.no_outward_bp,
            restore_bp: b.restore_bp,
            curve: match b.curve.kind {
                CurveKind::Linear => Curve::Linear {
                    num: b.curve.num,
                    den: b.curve.den,
                },
                CurveKind::Quadratic => Curve::Quadratic {
                    num: b.curve.num,
                    den: b.curve.den,
                },
            },
            max_step: b.max_step,
            restore_progress: b.restore_progress,
        })
        .collect();
    let mut rules = Vec::new();
    for r in file.policy.rules {
        let mut when = Vec::new();
        for c in r.when {
            when.push(Cond {
                dim: dim_of(&names, &c.dim)?,
                test: match c.test {
                    TestKind::DeviationAtLeast => Test::DeviationAtLeast(c.value),
                    TestKind::DeviationAtMost => Test::DeviationAtMost(c.value),
                    TestKind::SignedAtLeast => Test::SignedAtLeast(c.value),
                    TestKind::SignedAtMost => Test::SignedAtMost(c.value),
                },
            });
        }
        let effect = match r.effect {
            EffectFile::Adjust {
                target,
                side,
                delta,
            } => Effect::Adjust {
                target: dim_of(&names, &target)?,
                side: match side {
                    SideFile::Positive => Side::Positive,
                    SideFile::Negative => Side::Negative,
                    SideFile::Both => Side::Both,
                },
                delta,
            },
            EffectFile::ForceRestore { target, progress } => Effect::ForceRestore {
                target: dim_of(&names, &target)?,
                progress,
            },
        };
        rules.push(CouplingRule {
            id: r.id,
            when,
            effect,
        });
    }
    let mut rows = Vec::new();
    for r in file.catalog {
        let mut displacement = vec![0i64; names.len()];
        for (name, value) in &r.displacement {
            displacement[dim_of(&names, name)?] = *value;
        }
        rows.push(CatalogRow {
            action: r.action,
            description: r.description,
            displacement,
            tags: r.tags,
            act: r.act,
            material: r.material,
            must_assert: r.must_assert,
        });
    }
    Ok((
        Policy {
            version: file.policy.version,
            bands,
            rules,
            prohibited_tags: file.policy.prohibited_tags,
        },
        Catalog {
            rows,
            facts: file.facts,
            overclaim: file.overclaim_markers,
        },
    ))
}

/// A charter written back as the JSON its authority writes, so a store can carry every charter
/// that governed it. Reading it back gives the same policy and catalog.
pub fn charter_config_text(policy: &Policy, catalog: &Catalog) -> String {
    use serde_json::json;
    let name = |d: usize| {
        policy
            .bands
            .get(d)
            .map_or("?", |b| b.name.as_str())
            .to_string()
    };
    let bands: Vec<serde_json::Value> = policy
        .bands
        .iter()
        .map(|b| {
            let (kind, num, den) = match b.curve {
                Curve::Linear { num, den } => ("linear", num, den),
                Curve::Quadratic { num, den } => ("quadratic", num, den),
            };
            json!({
                "name": b.name, "center": b.center, "limit_pos": b.limit_pos,
                "limit_neg": b.limit_neg, "hard_pos": b.hard_pos, "hard_neg": b.hard_neg,
                "no_outward_bp": b.no_outward_bp, "restore_bp": b.restore_bp,
                "curve": {"kind": kind, "num": num, "den": den},
                "max_step": b.max_step, "restore_progress": b.restore_progress,
            })
        })
        .collect();
    let rules: Vec<serde_json::Value> = policy
        .rules
        .iter()
        .map(|r| {
            let when: Vec<serde_json::Value> = r
                .when
                .iter()
                .map(|c| {
                    let (test, value) = match c.test {
                        Test::DeviationAtLeast(x) => ("deviation_at_least", x),
                        Test::DeviationAtMost(x) => ("deviation_at_most", x),
                        Test::SignedAtLeast(x) => ("signed_at_least", x),
                        Test::SignedAtMost(x) => ("signed_at_most", x),
                    };
                    json!({"dim": name(c.dim), "test": test, "value": value})
                })
                .collect();
            let effect = match r.effect {
                Effect::Adjust { target, side, delta } => json!({
                    "kind": "adjust", "target": name(target), "delta": delta,
                    "side": match side { Side::Positive => "positive", Side::Negative => "negative", Side::Both => "both" },
                }),
                Effect::ForceRestore { target, progress } => {
                    json!({"kind": "force_restore", "target": name(target), "progress": progress})
                }
            };
            json!({"id": r.id, "when": when, "effect": effect})
        })
        .collect();
    let rows: Vec<serde_json::Value> = catalog
        .rows
        .iter()
        .map(|r| {
            let displacement: serde_json::Map<String, serde_json::Value> = r
                .displacement
                .iter()
                .enumerate()
                .filter(|(_, v)| **v != 0)
                .map(|(d, v)| (name(d), json!(v)))
                .collect();
            json!({
                "action": r.action, "description": r.description, "displacement": displacement,
                "tags": r.tags, "act": r.act, "material": r.material, "must_assert": r.must_assert,
            })
        })
        .collect();
    json!({
        "policy": {"version": policy.version, "prohibited_tags": policy.prohibited_tags,
                   "bands": bands, "rules": rules},
        "catalog": rows,
        "facts": catalog.facts,
        "overclaim_markers": catalog.overclaim,
    })
    .to_string()
}
