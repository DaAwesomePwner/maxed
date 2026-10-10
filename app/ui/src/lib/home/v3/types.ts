export type KpiCell = {
  key: string; label: string; value: string; of?: string | null; sub?: string | null;
  tone?: 'fame' | 'bad' | null; subTone?: 'good' | 'bad' | null; tooltip?: string | null; progress?: number | null;
};
export type BagOverflow = { bagType: number; count: number };
export type DamageRow = {
  rank: number | null; name: string | null; class_id: number | null;
  is_local: boolean; damage: number; dps: number | null; share: number | null;
};
