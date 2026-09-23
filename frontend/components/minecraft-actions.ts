import type { GameStatus, Instance } from '../types';
import { bridge } from '../services/bridge';
import { store } from '../state/store';
import { t } from '../i18n';
import { notify } from './ui';

/** Backend-owned actions: no game path, URL, hash, or Java arguments cross the webview boundary. */
export async function installMinecraft(instance: Instance): Promise<void> {
  const snapshot = await bridge.installMinecraft(instance.id);
  store.set(snapshot);
  notify(t('installed'));
}

export async function launchMinecraft(instance: Instance): Promise<GameStatus> {
  const status = await bridge.launchMinecraft(instance.id);
  // Launch also verifies/installs first, so refresh the persisted installation state.
  store.set(await bridge.bootstrap());
  notify(status.message);
  return status;
}

export async function stopMinecraft(instanceId: string): Promise<GameStatus> {
  const status = await bridge.stopMinecraft(instanceId);
  notify(status.message);
  return status;
}
