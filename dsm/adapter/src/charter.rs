//! What the policy authority signs: the bands, and the measurements of the actions.

use crate::commit;
use crate::enforcer::AdapterError;
use crate::Hash;
use dsm::crypto::sphincs::{self, SphincsVariant};
use ethical_band_engine::{Measurement, Policy, POSITION_BOUND};

#[derive(Clone)]
pub struct Keypair {
    pub public: Vec<u8>,
    pub secret: Vec<u8>,
}

impl std::fmt::Debug for Keypair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Keypair").finish_non_exhaustive()
    }
}

/// A SPHINCS+ key from a seed. The prototype's local keys come from fixed seeds so a run can be
/// reproduced byte for byte. They protect nothing and are not to be used for anything else.
pub fn keypair_from_seed(seed: &[u8; 32]) -> Result<Keypair, AdapterError> {
    let kp = sphincs::generate_keypair_from_seed(SphincsVariant::SPX256f, seed)
        .map_err(|e| AdapterError::Dsm(e.to_string()))?;
    Ok(Keypair {
        public: kp.public_key.clone(),
        secret: kp.secret_key.clone(),
    })
}

/// The independently supplied measurement of one action.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogRow {
    pub action: String,
    pub description: String,
    pub displacement: Vec<i64>,
    pub tags: Vec<String>,
    /// What the action does in the world, fixed by the charter.
    pub act: crate::act::Act,
}

impl CatalogRow {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let tags: Vec<&[u8]> = self.tags.iter().map(|t| t.as_bytes()).collect();
        commit::frame(&[
            self.action.as_bytes(),
            self.description.as_bytes(),
            &commit::integers(&self.displacement),
            &commit::frame(&tags),
            &self.act.canonical_bytes(),
        ])
    }

    pub fn measurement(&self) -> Measurement {
        Measurement {
            displacement: self.displacement.clone(),
            tags: self.tags.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Catalog {
    pub rows: Vec<CatalogRow>,
}

const MAX_ROW_TEXT: usize = 4_096;

impl Catalog {
    pub fn row(&self, action: &str) -> Option<&CatalogRow> {
        self.rows.iter().find(|r| r.action == action)
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        let rows: Vec<Vec<u8>> = self.rows.iter().map(CatalogRow::canonical_bytes).collect();
        let parts: Vec<&[u8]> = rows.iter().map(Vec::as_slice).collect();
        commit::frame(&parts)
    }

    fn validate(&self, dimensions: usize) -> bool {
        let mut seen = std::collections::BTreeSet::new();
        self.rows.iter().all(|r| {
            seen.insert(r.action.as_str())
                && !r.action.is_empty()
                && r.action.len() <= MAX_ROW_TEXT
                && r.description.len() <= MAX_ROW_TEXT
                && r.displacement.len() == dimensions
                && r.tags.iter().all(|t| t.len() <= MAX_ROW_TEXT)
                && r.act.is_well_formed()
                && r.displacement
                    .iter()
                    .all(|v| (-POSITION_BOUND..=POSITION_BOUND).contains(v))
        })
    }
}

/// A policy and its catalog under one authority signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedCharter {
    pub policy: Policy,
    pub catalog: Catalog,
    pub signature: Vec<u8>,
}

impl SignedCharter {
    pub fn sign(
        policy: Policy,
        catalog: Catalog,
        authority: &Keypair,
    ) -> Result<Self, AdapterError> {
        policy.validate().map_err(|_| AdapterError::BadCharter)?;
        if !catalog.validate(policy.bands.len()) {
            return Err(AdapterError::BadCharter);
        }
        let mut charter = SignedCharter {
            policy,
            catalog,
            signature: vec![],
        };
        charter.signature = sphincs::sphincs_sign(&authority.secret, &charter.message())
            .map_err(|e| AdapterError::Dsm(e.to_string()))?;
        Ok(charter)
    }

    /// Valid only under `trusted`, and only if it still validates.
    pub fn verify(&self, trusted: &[u8]) -> Result<(), AdapterError> {
        if self.policy.validate().is_err() || !self.catalog.validate(self.policy.bands.len()) {
            return Err(AdapterError::BadCharter);
        }
        match sphincs::sphincs_verify(trusted, &self.message(), &self.signature) {
            Ok(true) => Ok(()),
            _ => Err(AdapterError::BadCharter),
        }
    }

    pub fn policy_hash(&self) -> Hash {
        commit::policy_hash(&self.policy.canonical_bytes())
    }

    pub fn catalog_hash(&self) -> Hash {
        commit::catalog_hash(&self.catalog.canonical_bytes())
    }

    fn message(&self) -> Hash {
        commit::charter_message(&self.policy_hash(), &self.catalog_hash())
    }
}
