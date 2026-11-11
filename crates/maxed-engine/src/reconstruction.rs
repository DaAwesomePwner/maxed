//! Derived, bounded diagnostics. Never contains packet strings or identifiers.
use maxed_types::CoverageIssue;
use std::collections::BTreeMap;

macro_rules! reasons {
    ($($variant:ident => $label:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub enum ReconstructionReason { $($variant),+ }
        impl ReconstructionReason {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub fn name(self) -> &'static str {
                match self { $(Self::$variant => $label),+ }
            }
        }
    };
}
reasons! {
    AttemptedHits => "attemptedHits",
    ReconstructedHits => "reconstructedHits",
    OmittedHits => "omittedHits",
    ReconstructedZero => "reconstructedZero",
    AssumedCrucibleDefault => "assumedCrucibleDefault",
    AssumedBloodRitualDefault => "assumedBloodRitualDefault",
    AssumedAbilityEnchantDefault => "assumedAbilityEnchantDefault",
    AssumedWeaponEnchantDefault => "assumedWeaponEnchantDefault",
    AssumedScalingStatDefault => "assumedScalingStatDefault",
    AssumedTargetStatDefault => "assumedTargetStatDefault",
    MissingProjectile => "missingProjectile",
    MissingProjectileDefinition => "missingProjectileDefinition",
    MissingSeed => "missingSeed",
    UntrustedRng => "untrustedRng",
    MissingIdentity => "missingIdentity",
    StaleOwnerGeneration => "staleOwnerGeneration",
    ShotExpired => "shotExpired",
    OwnerLeftView => "ownerLeftView",
    ShotEvicted => "shotEvicted",
    MissingCondition => "missingCondition",
    MissingAttack => "missingAttack",
    MissingExaltation => "missingExaltation",
    InvalidAttack => "invalidAttack",
    UnknownEnchantMultiplier => "unknownEnchantMultiplier",
    MissingEnchantMetadata => "missingEnchantMetadata",
    MissingTarget => "missingTarget",
    InvalidDamage => "invalidDamage",
}
impl ReconstructionReason {
    pub(crate) fn issue(self) -> CoverageIssue {
        use ReconstructionReason::*;
        match self {
            MissingProjectile | ShotExpired | StaleOwnerGeneration | OwnerLeftView => {
                CoverageIssue::MissingProjectile
            }
            MissingProjectileDefinition | UnknownEnchantMultiplier | MissingEnchantMetadata => {
                CoverageIssue::MissingCatalog
            }
            MissingIdentity => CoverageIssue::MissingIdentity,
            MissingSeed => CoverageIssue::Midstream,
            ShotEvicted => CoverageIssue::ResourceLimit,
            _ => CoverageIssue::MissingStats,
        }
    }
}

#[derive(Default)]
pub(crate) struct ReconstructionCoverage {
    counts: BTreeMap<ReconstructionReason, u64>,
}
impl ReconstructionCoverage {
    pub fn increment(&mut self, reason: ReconstructionReason) {
        let n = self.counts.entry(reason).or_default();
        *n = n.saturating_add(1);
    }
    pub fn counts(&self) -> BTreeMap<&'static str, u64> {
        ReconstructionReason::ALL
            .iter()
            .map(|r| (r.name(), self.counts.get(r).copied().unwrap_or(0)))
            .collect()
    }
}
