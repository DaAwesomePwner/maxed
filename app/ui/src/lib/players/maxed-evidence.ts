type MaxedEvidence = { is_local: boolean; maxed_gated?: boolean };

// Remote players have no exalt split; local claims require the gated policy.
export function maxedClaimsSupported(evidence: MaxedEvidence): boolean {
  return !evidence.is_local || evidence.maxed_gated === true;
}

export function effectiveMaxed(profile: (MaxedEvidence & { maxed?: number | null }) | null): number | null {
  return profile && maxedClaimsSupported(profile) ? profile.maxed ?? null : null;
}
