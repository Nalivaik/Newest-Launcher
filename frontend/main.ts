import './styles/index.css';
import type { Route, Snapshot } from './types';
import { store, activeInstance, activeProfile } from './state/store';
import { bridge } from './services/bridge';
import { homePage } from './pages/home';
import { instancesPage } from './pages/instances';
import { catalogPage } from './pages/catalog';
import { settingsPage } from './pages/settings';
import { logsPage } from './pages/logs';
import { button, el, notifications, reportError, showDialog } from './components/ui';
import { icon } from './components/icons';
import { t } from './i18n';
import { accountDialog } from './components/account-dialog';

const root = document.querySelector<HTMLElement>('#root')!;
const routes: { id: Route; icon: string; group?: string }[] = [
  { id: 'home', icon: 'home' }, { id: 'instances', icon: 'instances', group: 'library' },
  { id: 'modpacks', icon: 'modpacks' }, { id: 'mods', icon: 'mods', group: 'content' },
  { id: 'packs', icon: 'packs' }, { id: 'shaders', icon: 'shaders' }, { id: 'logs', icon: 'logs' }, { id: 'settings', icon: 'settings' },
];
let route: Route = 'home';
let disposePage: (() => void) | undefined;
let loading = false;

const routeLabel = (id: Route) => t(id);
function navigate(next: Route): void { if (route === next && root.childElementCount) return; route = next; history.replaceState(null, '', `#${next}`); render(); }
function applySettings(snapshot: Snapshot): void {
  document.documentElement.dataset.theme = snapshot.settings.theme;
  document.documentElement.dataset.animations = String(snapshot.settings.animations);
  document.documentElement.dataset.transparency = String(snapshot.settings.transparency);
  document.documentElement.style.setProperty('--ui-scale', String(snapshot.settings.uiScale));
  document.documentElement.lang = snapshot.settings.language;
}
function buildSidebar(): HTMLElement {
  const aside = el('aside', 'sidebar');
  const brand = el('div', 'brand'); const brandCopy = el('div'); brandCopy.append(el('strong', '', 'Newest Launcher'), el('span', '', 'Minecraft desktop')); brand.append(el('div', 'cube mini-cube'), brandCopy);
  const profile = activeProfile();
  const account = button('', accountDialog, 'profile-card'); account.setAttribute('aria-label', t('accountTitle')); const head = el('div', 'player-head'); head.append(icon('user')); const accountCopy = el('div'); accountCopy.append(el('strong', '', profile?.username ?? t('noAccount')), el('span', '', profile?.kind === 'offline' ? t('offlineProfile') : t('accountStatus'))); account.append(head, accountCopy);
  const nav = el('nav', 'menu'); nav.setAttribute('aria-label', 'Main navigation');
  for (const item of routes) { if (item.group) nav.append(el('p', 'nav-label', t(item.group))); const control = button(routeLabel(item.id), () => navigate(item.id), `nav-item${route === item.id ? ' active' : ''}`); control.dataset.route = item.id; control.prepend(icon(item.icon)); nav.append(control); }
  const bottom = el('div', 'sidebar-bottom'); bottom.append(el('p', 'desktop-status', bridge.isDesktop ? 'Desktop core connected' : t('preview')));
  aside.append(brand, account, nav, bottom); return aside;
}
function globalSearch(): void {
  const body = el('div', 'global-search-dialog'); const input = el('input'); input.type = 'search'; input.placeholder = t('globalSearch'); const results = el('div', 'search-results'); const modal = showDialog(t('globalSearch'), body);
  const renderResults = () => { const query = input.value.trim().toLocaleLowerCase(); results.replaceChildren(); const candidates = [
    ...routes.map(item => ({ text: routeLabel(item.id), action: () => { modal.close(); navigate(item.id); } })),
    ...store.get().instances.map(instance => ({ text: `${instance.name} · ${instance.loader} ${instance.minecraftVersion}`, action: async () => { modal.close(); store.set(await bridge.selectInstance(instance.id)); navigate('home'); } })),
  ].filter(item => item.text.toLocaleLowerCase().includes(query)); if (!candidates.length) results.append(el('p', 'empty-state', t('empty'))); for (const candidate of candidates) results.append(button(candidate.text, candidate.action, 'search-result')); };
  input.addEventListener('input', renderResults); body.append(input, results); renderResults(); input.focus();
}
function notificationDialog(): void { const body = el('div', 'notification-list'); if (!notifications.length) body.append(el('p', 'empty-state', t('noNotifications'))); for (const item of notifications) { const row = el('article', item.error ? 'notification error' : 'notification'); row.append(el('p', '', item.message), el('time', '', new Intl.DateTimeFormat(undefined, { timeStyle: 'short' }).format(item.time))); body.append(row); } showDialog(t('notifications'), body); }
function buildTopbar(): HTMLElement {
  const header = el('header', 'topbar'); const crumb = el('div', 'crumb'); crumb.append(el('strong', '', routeLabel(route)), el('small', '', activeInstance()?.name ?? t('noInstance')));
  const actions = el('div', 'top-actions'); const search = button('', globalSearch, 'icon-button'); search.setAttribute('aria-label', t('globalSearch')); search.title = 'Ctrl+K'; search.append(icon('search')); const notices = button('', notificationDialog, 'icon-button'); notices.setAttribute('aria-label', t('notifications')); notices.append(icon('bell')); const settings = button('', () => navigate('settings'), 'icon-button'); settings.setAttribute('aria-label', t('settings')); settings.append(icon('settings')); actions.append(search, notices, settings); header.append(crumb, actions); return header;
}
function currentPage(): HTMLElement { disposePage?.(); disposePage = undefined; if (route === 'home') return homePage(navigate); if (route === 'instances') return instancesPage(); if (route === 'settings') return settingsPage(); if (route === 'logs') return logsPage(); const catalogue = catalogPage(route); disposePage = catalogue.dispose; return catalogue.element; }
function render(): void { applySettings(store.get()); const shell = el('main', 'shell'); const app = el('section', 'app'); const content = el('main', 'content'); content.id = 'content'; content.append(currentPage()); app.append(buildTopbar(), content); shell.append(buildSidebar(), app); root.replaceChildren(shell); }
async function bootstrap(): Promise<void> { loading = true; try { store.set(await bridge.bootstrap()); } catch (error) { reportError(error); } finally { loading = false; render(); } }
store.subscribe(() => { if (!loading) render(); });
document.addEventListener('keydown', event => { if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') { event.preventDefault(); globalSearch(); } if ((event.ctrlKey || event.metaKey) && event.key === ',') { event.preventDefault(); navigate('settings'); } if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'r' && ['mods', 'packs', 'shaders', 'modpacks'].includes(route)) { event.preventDefault(); document.querySelector<HTMLButtonElement>('#refreshCatalog')?.click(); } });
const hashRoute = location.hash.slice(1) as Route; if (routes.some(item => item.id === hashRoute)) route = hashRoute; void bootstrap();
