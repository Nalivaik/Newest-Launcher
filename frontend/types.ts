export type Loader = 'vanilla' | 'fabric' | 'forge' | 'neoforge' | 'quilt';
export type Language = 'ru' | 'en' | 'uk';
export interface Resolution { width: number; height: number; fullscreen: boolean }
export interface InstanceInput {
  name: string;
  minecraftVersion: string;
  loader: Loader;
  loaderVersion: string | null;
  javaPath: string | null;
  ramMb: number;
  jvmArgs: string[];
  resolution: Resolution;
}
export interface Instance extends InstanceInput {
  id: string;
  gameDirectory: string;
  icon: string | null;
  createdAt: string;
  lastPlayed: string | null;
  totalPlaytimeSeconds: number;
  installationState: 'not_installed' | 'installed' | 'corrupted';
  mods: InstalledContent[];
  resourcePacks: InstalledContent[];
  shaderPacks: InstalledContent[];
}
export interface InstalledContent {
  projectId: string;
  versionId: string;
  title: string;
  versionNumber: string;
  filename: string;
  sha512: string;
  installedAt: string;
}
export interface Settings {
  language: Language;
  theme: 'dark' | 'light' | 'system';
  animations: boolean;
  transparency: boolean;
  uiScale: number;
  defaultRamMb: number;
  concurrentDownloads: number;
}
export interface Snapshot {
  schemaVersion: number;
  instances: Instance[];
  trash: Instance[];
  activeInstanceId: string | null;
  profiles: Profile[];
  activeProfileId: string | null;
  settings: Settings;
  dataDirectory: string;
}
export interface Profile {
  id: string;
  kind: 'offline' | 'microsoft';
  username: string;
  uuid: string;
  skinUrl: string | null;
  createdAt: string;
  lastUsed: string | null;
}

export interface MicrosoftLoginChallenge {
  id: string;
  userCode: string;
  verificationUri: string;
  expiresIn: number;
}
export interface StorageUsage {
  instancesBytes: number; cacheBytes: number; trashBytes: number; freeBytes: number; totalBytes: number;
}
export interface LogEntry { timestamp: string; level: string; event: string; message: string }
export interface GameStatus {
  instanceId: string | null;
  phase: 'idle' | 'installing' | 'installed' | 'launching' | 'running' | 'stopping' | 'failed';
  message: string;
  completedFiles: number;
  totalFiles: number;
  completedBytes: number;
  totalBytes: number;
  pid: number | null;
  lastError: string | null;
}
export interface GameVersion { version: string; version_type: string; date: string }
export type ProjectType = 'mod' | 'resourcepack' | 'shader' | 'modpack';
export interface ProjectHit {
  project_id: string; slug: string; title: string; description: string; author: string;
  icon_url: string | null; downloads: number; categories: string[]; versions: string[];
  project_type: ProjectType;
}
export interface SearchResults { hits: ProjectHit[]; total_hits: number; offset: number; limit: number }
export type Route = 'home' | 'instances' | 'mods' | 'packs' | 'shaders' | 'modpacks' | 'settings' | 'logs';
