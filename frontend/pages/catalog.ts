import type { ProjectType, Route } from '../types';
import { activeInstance, store } from '../state/store';
import { t } from '../i18n';
import { bridge } from '../services/bridge';
import { clearCatalogueCache, gameVersions, searchProjects } from '../services/catalog';
import { button, el, notify, reportError, skeletons } from '../components/ui';
import { icon } from '../components/icons';

const projectTypes: Partial<Record<Route, ProjectType>> = { mods: 'mod', packs: 'resourcepack', shaders: 'shader', modpacks: 'modpack' };
export function catalogPage(route: Route): { element: HTMLElement; dispose(): void; refresh(): void } {
  const type = projectTypes[route]!;
  const instance = activeInstance();
  const page = el('section');
  const head = el('div', 'library-head');
  const title = el('div');
  title.append(el('p', 'eyebrow', 'MODRINTH'), el('h1', '', t(route)), el('p', '', t('catalogNote')));
  const refresh = button(t('refresh'), () => { clearCatalogueCache(); void load(); }, 'soft-button');
  head.append(title, refresh);
  const toolbar = el('form', 'library-toolbar catalog-toolbar');
  toolbar.addEventListener('submit', event => event.preventDefault());
  const search = el('input');
  search.type = 'search';
  search.placeholder = t('searchCatalog');
  search.setAttribute('aria-label', t('searchCatalog'));
  const searchLabel = el('label', 'search');
  searchLabel.append(icon('search'), search);
  const version = el('select');
  version.setAttribute('aria-label', 'Minecraft version');
  version.add(new Option(t('allVersions'), ''));
  if (instance) version.add(new Option(instance.minecraftVersion, instance.minecraftVersion, true, true));
  const loader = el('select');
  loader.setAttribute('aria-label', 'Loader');
  loader.add(new Option(t('allLoaders'), ''));
  for (const value of ['fabric', 'forge', 'neoforge', 'quilt']) loader.add(new Option(value, value));
  if (instance && instance.loader !== 'vanilla') loader.value = instance.loader;
  loader.hidden = type !== 'mod';
  toolbar.append(searchLabel, version, loader);
  const results = el('div');
  const footer = el('div', 'pagination');
  const count = el('span', 'muted');
  let offset = 0;
  let generation = 0;
  let disposed = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const prev = button(t('previous'), () => { offset = Math.max(0, offset - 18); void load(); });
  const next = button(t('next'), () => { offset += 18; void load(); });
  footer.append(prev, count, next);
  page.append(head, toolbar, results, footer);

  async function load(): Promise<void> {
    const request = ++generation;
    prev.disabled = next.disabled = true;
    results.replaceChildren(skeletons());
    try {
      const result = await searchProjects({ type, query: search.value.trim(), version: version.value,
        loader: loader.value, offset, sort: search.value.trim() ? 'relevance' : 'downloads' });
      if (disposed || request !== generation) return;
      const grid = el('div', 'asset-grid');
      for (const hit of result.hits) {
        const card = el('article', 'asset-card');
        const cover = el('div', 'asset-cover');
        if (hit.icon_url?.startsWith('https://cdn.modrinth.com/')) {
          const image = el('img'); image.src = hit.icon_url; image.alt = ''; image.loading = 'lazy';
          image.addEventListener('error', () => image.replaceWith(icon('instances')), { once: true });
          cover.append(image);
        } else cover.append(icon('instances'));
        const body = el('div', 'asset-body');
        const actions = el('div', 'asset-actions');
        if (type !== 'modpack') {
          const install = button(t('installContent'), () => {
            if (!instance || !bridge.isDesktop) return;
            install.disabled = true;
            install.textContent = t('installing');
            void bridge.installModrinthContent(instance.id, hit.project_id, type).then(snapshot => {
              store.set(snapshot);
              notify(t('contentInstalled'));
            }).catch(error => reportError(error)).finally(() => {
              install.disabled = false;
              install.textContent = t('installContent');
            });
          }, 'soft-button');
          install.disabled = !instance || !bridge.isDesktop;
          if (!instance) install.title = t('chooseInstanceForContent');
          if (!bridge.isDesktop) install.title = t('preview');
          actions.append(install);
        }
        actions.append(button(t('projectPage'), () => bridge.openExternal(`https://modrinth.com/${type}/${encodeURIComponent(hit.slug)}`)));
        body.append(el('h3', '', hit.title), el('p', 'asset-description', hit.description),
          el('p', 'asset-author', hit.author),
          el('span', 'download-count', `${new Intl.NumberFormat().format(hit.downloads)} downloads`), actions);
        card.append(cover, body);
        grid.append(card);
      }
      results.replaceChildren(result.hits.length ? grid : el('p', 'empty-state', t('emptyCatalog')));
      count.textContent = `${result.total_hits ? offset + 1 : 0}–${offset + result.hits.length} / ${result.total_hits}`;
      prev.disabled = offset === 0;
      next.disabled = offset + result.hits.length >= result.total_hits;
    } catch (error) {
      if (disposed || request !== generation) return;
      const panel = el('div', 'empty-state');
      panel.append(el('p', 'error-text', error instanceof Error ? error.message : String(error)),
        button(t('retry'), () => { void load(); }));
      results.replaceChildren(panel);
      count.textContent = '';
    }
  }
  search.addEventListener('input', () => {
    ++generation;
    clearTimeout(timer);
    timer = setTimeout(() => { offset = 0; void load(); }, 350);
  });
  for (const select of [version, loader]) select.addEventListener('change', () => {
    clearTimeout(timer); offset = 0; void load();
  });
  void gameVersions().then(versions => {
    if (disposed) return;
    for (const value of versions) {
      if (value.version !== instance?.minecraftVersion) version.add(new Option(value.version, value.version));
    }
  }).catch(() => { /* Search and manually selected instance remain usable offline. */ });
  void load();
  return { element: page, dispose: () => { disposed = true; ++generation; clearTimeout(timer); }, refresh: () => { clearCatalogueCache(); void load(); } };
}
