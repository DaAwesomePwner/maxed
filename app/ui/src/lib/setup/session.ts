import { writable } from 'svelte/store';
import type { Client } from '../bridge/client';
import { subscribeCaptureStatus } from '../bridge/capture-status';
import type { SetupStatus } from '../bindings/SetupStatus';
import type { CaptureStatus } from '../bindings/CaptureStatus';
import type { AppSettings } from '../bindings/AppSettings';
import type { AdapterInfo } from '../bindings/AdapterInfo';
import type { ShellConnection } from '../shell/context';

type Errors = Record<'setup' | 'capture' | 'settings' | 'adapters' | 'action' | 'assets', string>;
export interface SetupView {
  setup: SetupStatus | null;
  capture: CaptureStatus | null;
  settings: AppSettings | null;
  adapters: AdapterInfo[] | null;
  errors: Errors;
  busy: boolean;
  extracting: boolean;
  previousAssets: boolean;
}

/** One mounted route owns this session. No idle polling or persisted completion flag. */
export function createSetupSession(client: Client, preview = false, connection?: ShellConnection) {
  let value: SetupView = {
    setup: null, capture: null, settings: null, adapters: null,
    errors: { setup: '', capture: '', settings: '', adapters: '', action: '', assets: '' },
    busy: false, extracting: false, previousAssets: false,
  };
  const store = writable(value);
  let disposed = false, started = false, setupRequest = 0, assetRequest = 0, settingsRequest = 0, adaptersRequest = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let releaseCapture = () => {};
  let captureActionFailed = false;
  const publish = (patch: Partial<SetupView>) => {
    if (disposed) return;
    value = { ...value, ...patch }; store.set(value);
  };
  const error = (key: keyof Errors, message: string) => publish({ errors: { ...value.errors, [key]: message } });
  function schedule() {
    clearTimeout(timer);
    if (!disposed && !value.errors.setup && (value.extracting || value.busy || value.setup?.assets === 'extracting' || value.setup?.assets === 'finding')) {
      timer = setTimeout(() => { void refreshSetup(); }, 250);
    }
  }
  async function refreshSetup({ preserveAssetError = false, retry = false }: { preserveAssetError?: boolean; retry?: boolean } = {}) {
    clearTimeout(timer);
    const request = ++setupRequest;
    try {
      const setup = await (retry ? client.querySetup({ retry: true }) : client.querySetup());
      if (disposed || request !== setupRequest) return;
      const becameReady = value.setup !== null && value.setup.assets !== 'ready' && setup.assets === 'ready';
      publish({ setup, previousAssets: value.previousAssets || setup.assets === 'ready' });
      error('setup', '');
      // A retained ready cache does not prove a rejected selection succeeded.
      if (becameReady && !preserveAssetError) error('assets', '');
    } catch {
      if (disposed || request !== setupRequest) return;
      publish({ setup: null });
      error('setup', 'Couldn’t check setup. Retry.');
    }
    if (request === setupRequest) schedule();
  }
  async function refreshSettings() {
    const request = ++settingsRequest;
    try {
      const settings = await client.querySettings();
      if (disposed || request !== settingsRequest) return;
      publish({ settings }); error('settings', '');
    } catch { if (request === settingsRequest) error('settings', 'Couldn’t load settings. Retry.'); }
  }
  async function refreshAdapters() {
    if (preview || disposed) return;
    const request = ++adaptersRequest;
    try {
      const adapters = await client.listAdapters();
      if (disposed || request !== adaptersRequest) return;
      publish({ adapters }); error('adapters', '');
    } catch {
      if (request !== adaptersRequest) return;
      publish({ adapters: null }); error('adapters', 'Couldn’t list adapters. Retry.');
    }
  }
  function connectCapture(reconnect = true) {
    releaseCapture();
    const receive = (capture: CaptureStatus | null) => {
      if (!capture) { publish({ capture: null }); return; }
      const changed = capture.state !== value.capture?.state;
      publish({ capture, ...(changed && value.setup ? { setup: { ...value.setup, live_confirmed: false } } : {}) });
      error('capture', '');
      if (changed) {
        if (captureActionFailed) { error('action', ''); captureActionFailed = false; }
        void refreshSetup();
      }
    };
    if (connection) {
      releaseCapture = connection.capture.subscribe(receive);
      if (reconnect) connection.reconnect();
      return;
    }
    releaseCapture = subscribeCaptureStatus(client.onCapture, client.queryCapture, receive, () => {
      publish({ capture: null }); error('capture', 'Couldn’t load capture. Retry.');
    });
  }
  async function action(command: () => Promise<void>, failure: string, { duringExtraction = false, captureAction = false }: { duringExtraction?: boolean; captureAction?: boolean } = {}) {
    if (disposed || preview || value.busy || (value.extracting && !duringExtraction)) return false;
    captureActionFailed = false;
    publish({ busy: true }); error('action', ''); schedule();
    let accepted = false;
    try { await command(); accepted = true; }
    catch { captureActionFailed = captureAction; error('action', failure); }
    finally {
      if (!disposed) await Promise.all([refreshSetup(), refreshSettings()]);
      publish({ busy: false }); schedule();
    }
    return accepted && !disposed;
  }
  async function selectAssets(path: string) {
    if (disposed || preview || value.busy) return false;
    if (!path.trim()) { error('assets', 'Enter an assets path.'); return false; }
    const request = ++assetRequest;
    publish({ extracting: true }); error('assets', ''); schedule();
    let accepted = false;
    try { await client.setAssetsPath(path.trim()); accepted = true; }
    catch {
      if (request === assetRequest) error('assets', 'Couldn’t extract assets. Retry.');
    } finally {
      // Replacing the selection cancels the native generation. Its late rejection
      // must not erase progress or show an error for the replacement.
      if (!disposed && request === assetRequest) {
        publish({ extracting: false });
        await Promise.all([refreshSetup({ preserveAssetError: !accepted }), refreshSettings()]);
      }
    }
    return accepted && !disposed && request === assetRequest && !value.errors.assets;
  }
  return {
    subscribe: store.subscribe, refreshSetup, refreshSettings, refreshAdapters, connectCapture, action, selectAssets,
    start() {
      if (started || disposed) return;
      started = true; connectCapture(false);
      void refreshSetup(); void refreshSettings(); void refreshAdapters();
    },
    dispose() { disposed = true; setupRequest++; assetRequest++; clearTimeout(timer); releaseCapture(); },
  };
}
export type SetupSession = ReturnType<typeof createSetupSession>;
