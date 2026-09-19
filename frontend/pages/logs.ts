import { bridge } from '../services/bridge';
import { button, el, notify } from '../components/ui';
import { t } from '../i18n';
import type { LogEntry } from '../types';

export function logsPage(): HTMLElement {
  const page = el('section');
  const head = el('div', 'library-head');
  const title = el('div');
  title.append(el('h1', '', t('logs')), el('p', '', t('logInfo')));
  const viewer = el('pre', 'log-viewer');
  viewer.tabIndex = 0;
  const search = el('input'); search.type = 'search'; search.placeholder = t('globalSearch');
  search.setAttribute('aria-label', 'Search logs');
  let logs: LogEntry[] = [];
  const render = () => { viewer.textContent = logs.filter(entry => JSON.stringify(entry).toLowerCase().includes(search.value.toLowerCase()))
    .map(entry => `${entry.timestamp} [${entry.level}] ${entry.event} ${entry.message}`).join('\n') || t('logsEmpty'); };
  const refresh = async () => { logs = await bridge.readLogs(); render(); };
  const actions = el('div', 'section-actions');
  actions.append(button(t('refresh'), refresh), button(t('copy'), async () => { await navigator.clipboard.writeText(viewer.textContent ?? ''); notify(t('copied')); }),
    button(t('folder'), () => bridge.openFolder('logs')));
  head.append(title, actions);
  page.append(head, search, viewer);
  search.addEventListener('input', render);
  if (bridge.isDesktop) void refresh().catch(error => { viewer.textContent = String(error); });
  else viewer.textContent = t('preview');
  return page;
}
