import type { DamageBucket } from '../../bindings/DamageBucket';
import type { DropView } from '../../bindings/DropView';
import type { EnchantView } from '../../bindings/EnchantView';
import type { FightView } from '../../bindings/FightView';
import type { InstanceEventView } from '../../bindings/InstanceEventView';
import type { PlayerDamage } from '../../bindings/PlayerDamage';
import type { PlayerSeries } from '../../bindings/PlayerSeries';
import type { HomeVisuals } from '../visuals';

// Visit history has no charts or player lists; detailed focus stays in the rolling window.
export type RealmKillSummary = Pick<FightView, 'id' | 'session_id' | 'run_id' | 'target_type'
  | 'target_kind' | 'started' | 'ended' | 'kill'> & { self: PlayerDamage | null; partial: boolean };

export type HomeScreen = 'dungeon' | 'realm' | 'nexus';
export interface HomeLiveWindow {
  streamId: string | null;
  visitKey: string | null; // first accepted entry for this realm instance or dungeon entry
  visitStarted: string | null;
  realmPeriods?: { started: string; ended: string | null; fame: number | null }[];
  runId: string | null;
  focusedFightId?: string | null;
  observedAt?: string;
  fightSequences?: Record<string, string>;
  realmKills?: RealmKillSummary[];
  realmAnnouncements?: InstanceEventView[];
  fights: FightView[];
  drops: DropView[];
  events: InstanceEventView[];
  partial: boolean;
  reasons: string[];
}
export interface HomeLootProps {
  drops: DropView[];
  visuals: HomeVisuals;
  partial: boolean;
  reasons: string[];
  title: string;
}
export interface ObservedLootItem {
  drop: DropView;
  itemType: number;
  tier: string | null;
  /** Null for legacy item lists without per-slot enchant evidence. */
  enchant: EnchantView | null;
}
export interface LootView {
  drops: DropView[];
  bestUt: ObservedLootItem | null;
  bestSt: ObservedLootItem | null;
  bagCount: number;
  utCount: number;
  stCount: number;
  unclassifiedCount: number;
  partial: boolean;
  reasons: string[];
}
export type LootViewProjector = (input: HomeLootProps) => LootView;
export interface DamageChartProps {
  buckets: DamageBucket[];
  playerSeries: PlayerSeries[] | null;
  selectedPlayerId: string | null;
}
