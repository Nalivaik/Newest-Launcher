import type { Snapshot } from '../types';

export const emptySnapshot = (): Snapshot => ({
  schemaVersion: 2, instances: [], trash: [], activeInstanceId: null, profiles: [], activeProfileId: null, dataDirectory: '',
  settings: { language: 'ru', theme: 'dark', animations: true, transparency: true,
    uiScale: 1, defaultRamMb: 4096, concurrentDownloads: 4 },
});

let snapshot = emptySnapshot();
const listeners = new Set<() => void>();
export const store = {
  get: (): Snapshot => snapshot,
  set(next: Snapshot): void {
    snapshot = next;
    listeners.forEach(listener => listener());
  },
  subscribe(listener: () => void): () => void {
    listeners.add(listener);
    return () => { listeners.delete(listener); };
  },
};
export const activeInstance = () => snapshot.instances.find(instance => instance.id === snapshot.activeInstanceId);
export const activeProfile = () => snapshot.profiles.find(profile => profile.id === snapshot.activeProfileId);
