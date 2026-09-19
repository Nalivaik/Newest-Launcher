import { bridge } from './bridge';
import type { GameVersion, ProjectType, SearchResults } from '../types';

const TTL = 5 * 60_000;
const MAX_ENTRIES = 64;
const cache = new Map<string, { time: number; value: unknown }>();
const pending = new Map<string, Promise<unknown>>();

async function request<T>(path: string): Promise<T> {
  const cached = cache.get(path);
  if (cached && Date.now() - cached.time < TTL) return cached.value as T;
  if (pending.has(path)) return pending.get(path) as Promise<T>;
  const promise = (async () => {
    let value: T;
    if (bridge.isDesktop) value = await bridge.metadata<T>(path);
    else {
      const controller = new AbortController();
      const timeout = setTimeout(() => controller.abort(), 25_000);
      try {
        const response = await fetch(`https://api.modrinth.com/v2${path}`, { signal: controller.signal });
        if (!response.ok) throw new Error(`Modrinth: HTTP ${response.status}`);
        value = await response.json() as T;
      } finally { clearTimeout(timeout); }
    }
    if (cache.size >= MAX_ENTRIES) cache.delete(cache.keys().next().value!);
    cache.set(path, { time: Date.now(), value });
    return value;
  })();
  pending.set(path, promise);
  try { return await promise; } finally { pending.delete(path); }
}

export function gameVersions(): Promise<GameVersion[]> { return request('/tag/game_version'); }
export interface SearchInput { type: ProjectType; query: string; version: string; loader: string; offset: number; sort: string }
export function searchPath(input: SearchInput): string {
  const facets = [[`project_type:${input.type}`]];
  if (input.version) facets.push([`versions:${input.version}`]);
  if (input.type === 'mod' && input.loader && input.loader !== 'vanilla') facets.push([`categories:${input.loader}`]);
  const params = new URLSearchParams({ facets: JSON.stringify(facets), query: input.query,
    offset: String(input.offset), limit: '18', index: input.sort });
  return `/search?${params}`;
}
export function searchProjects(input: SearchInput): Promise<SearchResults> { return request(searchPath(input)); }
export function clearCatalogueCache(): void { cache.clear(); }
