import type { SetupStatus } from '../bindings/SetupStatus';
import type { CaptureStatus } from '../bindings/CaptureStatus';
import type { ExtractionStage } from '../bindings/ExtractionStage';

export const stages: Record<ExtractionStage, string> = {
  fingerprinting: 'Fingerprinting', reading: 'Reading', catalog: 'Catalog', sprites: 'Sprites', publishing: 'Publishing',
};
export function npcapText(setup: SetupStatus | null) {
  if (!setup) return 'Unchecked';
  if (setup.npcap === 'permission_denied' || setup.access === 'denied') return 'Access denied';
  switch (setup.npcap) {
    case 'missing': return 'Install Npcap';
    case 'incompatible': return 'Repair Npcap';
    case 'compatible': return setup.access === 'allowed' ? 'Ready' : 'Checking access';
    case 'failed': return 'Couldn’t check Npcap';
    default: return 'Unchecked';
  }
}
export function assetsText(setup: SetupStatus | null) {
  if (!setup) return 'Loading';
  switch (setup.assets) {
    case 'finding': return 'Finding client';
    case 'extracting': return `Extracting${setup.extraction_stage ? `: ${stages[setup.extraction_stage]}` : ''}`;
    case 'ready': return setup.client_found ? 'Assets ready' : 'Select client';
    case 'unsupported': return 'Unsupported client';
    case 'failed': return 'Couldn’t extract assets';
    default: return 'Select client';
  }
}
/** One failure per line: drop the chained "Retry." and the closing period of a one-sentence message. */
export function errorLine(message: string) {
  return message.replace(/\.\s*Retry\.$/, '');
}
export function setupProgress(setup: SetupStatus | null, capture: CaptureStatus | null, preview: boolean, pending = false) {
  const npcap = !preview && !setup?.demo && setup?.npcap === 'compatible' && setup.access === 'allowed';
  // Cached assets alone do not establish that the selected client was found.
  const assets = setup?.client_found === true && setup.assets === 'ready';
  const live = !pending && npcap && assets && setup?.live_confirmed === true && capture?.state === 'Live';
  return { npcap, assets, live, current: !npcap ? 1 : !assets ? 2 : 3 };
}
