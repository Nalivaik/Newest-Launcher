import { invoke, isTauri } from '@tauri-apps/api/core';
import type { GameStatus, InstanceInput, ProjectType, Settings, Snapshot, StorageUsage, LogEntry } from '../types';
import { emptySnapshot } from '../state/store';

async function command<T>(name: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error('Для работы с файлами запустите desktop-приложение: npm run desktop:dev');
  try { return await invoke<T>(name, args); }
  catch (error) { throw new Error(typeof error === 'string' ? error : 'Не удалось выполнить операцию. Проверьте журнал лаунчера.'); }
}

export const bridge = {
  isDesktop: isTauri(),
  bootstrap: (): Promise<Snapshot> => isTauri() ? command('bootstrap') : Promise.resolve(emptySnapshot()),
  createInstance: (input: InstanceInput): Promise<Snapshot> => command('create_instance', { input }),
  updateInstance: (id: string, input: InstanceInput): Promise<Snapshot> => command('update_instance', { id, input }),
  cloneInstance: (id: string, name: string): Promise<Snapshot> => command('clone_instance', { id, name }),
  deleteInstance: (id: string): Promise<Snapshot> => command('delete_instance', { id }),
  restoreInstance: (id: string): Promise<Snapshot> => command('restore_instance', { id }),
  selectInstance: (id: string): Promise<Snapshot> => command('select_instance', { id }),
  createOfflineProfile: (username: string): Promise<Snapshot> => command('create_offline_profile', { username }),
  selectProfile: (id: string): Promise<Snapshot> => command('select_profile', { id }),
  deleteOfflineProfile: (id: string): Promise<Snapshot> => command('delete_offline_profile', { id }),
  installMinecraft: (id: string): Promise<Snapshot> => command('install_minecraft', { id }),
  installModrinthContent: (id: string, projectId: string, contentType: Extract<ProjectType, 'mod' | 'resourcepack' | 'shader'>): Promise<Snapshot> =>
    command('install_modrinth_content', { id, projectId, contentType }),
  launchMinecraft: (id: string): Promise<GameStatus> => command('launch_minecraft', { id }),
  stopMinecraft: (): Promise<GameStatus> => command('stop_minecraft'),
  minecraftStatus: (): Promise<GameStatus> => command('minecraft_status'),
  saveSettings: (settings: Settings): Promise<Snapshot> => command('save_settings', { settings }),
  importInstance: (): Promise<Snapshot | null> => command('import_instance'),
  exportInstance: (id: string): Promise<boolean> => command('export_instance', { id }),
  openFolder: (kind: string, instanceId?: string): Promise<void> => command('open_folder', { kind, instanceId: instanceId ?? null }),
  storageUsage: (): Promise<StorageUsage> => command('storage_usage'),
  readLogs: (): Promise<LogEntry[]> => command('read_logs'),
  openExternal: async (url: string): Promise<void> => {
    const parsed = new URL(url);
    if (parsed.protocol !== 'https:' || !['modrinth.com', 'www.minecraft.net'].includes(parsed.hostname)) {
      throw new Error('Этот адрес не разрешён.');
    }
    if (isTauri()) await command('open_external', { url });
    else window.open(url, '_blank', 'noopener,noreferrer');
  },
  metadata: <T>(path: string): Promise<T> => command('modrinth_metadata', { path }),
};
