import type { LegacyPreviewMode, PreviewMode } from './scenario';

export const stressCases: ReadonlyArray<{
  mode: LegacyPreviewMode; label: string; query: string; ended: boolean; stress: boolean; participants: number; target: string;
}> = [
  { mode: 'nexus', label: 'nexus-stress',
    query: '?preview=nexus&variant=stress', ended: false, stress: true, participants: 47, target: 'Lord of the Lost Lands' },
  { mode: 'dungeon', label: 'dungeon-stress',
    query: '?preview=dungeon&variant=stress', ended: false, stress: true, participants: 47, target: 'Lord of the Lost Lands' },
  { mode: 'realm', label: 'realm-stress',
    query: '?preview=realm&variant=stress', ended: false, stress: true, participants: 47, target: 'Lord of the Lost Lands' },
  { mode: 'realm', label: 'realm-busy',
    query: '?preview=realm&variant=busy', ended: true, stress: false, participants: 85, target: 'Willow Behemoth' },
];

// Exact retained copy: Nexus Task 8, Dungeon Task 6, Realm Task 9.
export const debugText: Record<LegacyPreviewMode, readonly string[]> = {
  nexus: [
    'Session scope ends after 30 minutes without game traffic.',
    'Known baselines',
  ],
  dungeon: [
    'Players are estimated. Departure causes are unconfirmed.',
    'Loot counts are appearances in bags, not stack sizes or items picked up.',
  ],
  realm: [
    'Estimated population',
    'Loot counts are appearances in bags, not stack sizes or items picked up.',
  ],
};

export const v4Cases: ReadonlyArray<{
  mode: PreviewMode; label: string; query: string; root: string; target: string;
  stress: false; rank: number | null; participants: number; partyRecorded: boolean;
  debugText: readonly string[];
}> = [
  { mode: 'home-v4-dungeon', label: 'home-v4-dungeon', query: '?preview=home-v4-dungeon',
    root: '.dungeon-screen', target: 'Moonlit Warden', stress: false, rank: 2, participants: 9,
    partyRecorded: true, debugText: debugText.dungeon },
  { mode: 'home-v4-realm', label: 'home-v4-realm', query: '?preview=home-v4-realm',
    root: '.realm-screen', target: 'Starfall Guardian', stress: false, rank: 9, participants: 37,
    partyRecorded: true, debugText: debugText.realm },
  { mode: 'home-v4-realm', label: 'home-v4-realm-legacy', query: '?preview=home-v4-realm&variant=legacy',
    root: '.realm-screen', target: 'Willow Behemoth', stress: false, rank: 9, participants: 37,
    partyRecorded: false, debugText: debugText.realm },
  { mode: 'home-v4-nexus', label: 'home-v4-nexus', query: '?preview=home-v4-nexus',
    root: '.nexus-home', target: 'Nexus', stress: false, rank: null, participants: 0,
    partyRecorded: false, debugText: debugText.nexus },
  { mode: 'home-v4-recap', label: 'home-v4-recap', query: '?preview=home-v4-recap',
    root: '.recap-screen', target: 'CLEARED', stress: false, rank: null, participants: 8,
    partyRecorded: false, debugText: [] },
  { mode: 'home-v4-recap-died', label: 'home-v4-recap-died', query: '?preview=home-v4-recap-died',
    root: '.recap-screen .death-banner', target: 'DIED AT', stress: false, rank: null, participants: 8,
    partyRecorded: false, debugText: [] },
  { mode: 'home-v4-recap-realm', label: 'home-v4-recap-realm', query: '?preview=home-v4-recap-realm',
    root: '.visit-recap-screen', target: 'REALM VISIT', stress: false, rank: null, participants: 8,
    partyRecorded: false, debugText: [] },
];

export const stressRail = {
  name: 'TestLongHeroXX',
  hp: '1250/1385|27 (+310)',
  mp: '52/1105|3',
  fame: '125340',
  att: '150|25 (+105)',
  vit: '140|40 (+100)',
  spd: '99 (−24)',
} as const;

// Busy Realm publishes only ended encounters, so shared focus retains realm-retained-0
// (seven players, local rank 1), while its final snapshot fight has 85 participants.
// The reviewed busy variants retain their original rail and participant fixtures.
export const busyRail = {
  name: 'TestMage', hp: '725/800 (+80)', mp: '210/385 (+35)', fame: '1680',
  att: '82 (+7)', vit: '55 (+5)', spd: '52 (+2)',
} as const;

export const announcementDebugText = 'Contribution unavailable - announcement has no exact fight link.';
export function caseDebugText(scenario: typeof stressCases[number]): readonly string[] {
  return scenario.mode === 'realm' && scenario.stress
    ? [...debugText.realm, announcementDebugText] : debugText[scenario.mode];
}
