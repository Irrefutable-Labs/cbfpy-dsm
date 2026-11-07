//! The human-written charter: the protected-life rules, the catalog, and the fact register.
//! Integers only, unknown fields refused.

use crate::charter::{Catalog, CatalogRow, Policy};
use crate::enforcer::AdapterError;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    policy: Policy,
    catalog: Vec<CatalogRow>,
    #[serde(default)]
    facts: Vec<crate::honesty::Fact>,
    #[serde(default)]
    overclaim_markers: Vec<String>,
}

pub fn parse_charter_config(text: &str) -> Result<(Policy, Catalog), AdapterError> {
    let file: ConfigFile =
        serde_json::from_str(text).map_err(|e| AdapterError::Config(e.to_string()))?;
    Ok((
        file.policy,
        Catalog {
            rows: file.catalog,
            facts: file.facts,
            overclaim: file.overclaim_markers,
        },
    ))
}

/// A charter written back as the JSON its authority writes, so a store can carry every charter
/// that governed it. Reading it back gives the same rules and catalog.
pub fn charter_config_text(policy: &Policy, catalog: &Catalog) -> String {
    serde_json::to_string(&ConfigFile {
        policy: policy.clone(),
        catalog: catalog.rows.clone(),
        facts: catalog.facts.clone(),
        overclaim_markers: catalog.overclaim.clone(),
    })
    .unwrap_or_default()
}
