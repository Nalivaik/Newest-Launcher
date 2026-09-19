import { activeInstance, activeProfile, store } from '../state/store';
import { bridge } from '../services/bridge';
import { button, el, reportError } from '../components/ui';
import { icon } from '../components/icons';
import { instanceForm } from '../components/instance-form';
import { installMinecraft, launchMinecraft, stopMinecraft } from '../components/minecraft-actions';
import { t } from '../i18n';
import type { GameStatus, Instance, Route } from '../types';

function installationLabel(instance: Instance): string {
  if (instance.installationState === 'installed') return t('installed');
  if (instance.installationState === 'corrupted') return t('corrupted');
  return t('notInstalled');
}

export function homePage(navigate: (route: Route) => void): HTMLElement {
  const template = document.querySelector<HTMLTemplateElement>('#home-template')!;
  const page = el('section', 'home-page');
  page.append(template.content.cloneNode(true));
  page.querySelectorAll<HTMLElement>('[data-text]').forEach(node => { node.textContent = t(node.dataset.text!); });
  page.querySelectorAll<HTMLElement>('[data-icon]').forEach(node => node.replaceChildren(icon(node.dataset.icon!)));
  const instance = activeInstance();
  const profile = activeProfile();
  page.querySelector('#versionText')!.textContent = instance ? `${instance.loader} ${instance.minecraftVersion}` : t('choose');
  page.querySelector('#playVersion')!.textContent = instance?.name ?? t('noInstance');
  page.querySelector('#instanceState')!.textContent = instance ? installationLabel(instance) : 'NEWEST LAUNCHER';
  page.querySelector('#instanceSummary')!.textContent = instance
    ? `${instance.loader} ${instance.minecraftVersion} · ${instance.mods.length} mods · ${profile?.kind === 'offline' ? 'offline' : 'no Microsoft profile'}`
    : t('noLibrary');
  page.querySelector<HTMLButtonElement>('#versionButton')!.onclick = () => navigate('instances');
  page.querySelector<HTMLButtonElement>('#playOptions')!.onclick = () => { void instanceForm(instance); };
  const play = page.querySelector<HTMLButtonElement>('#playButton')!;
  const launchNote = page.querySelector<HTMLElement>('#launchNote')!;
  const playText = play.querySelector<HTMLElement>('[data-text]')!;
  const stop = button(t('stop'), async () => {
    const status = await stopMinecraft();
    renderStatus(status);
  }, 'soft-button');
  stop.hidden = true;
  const renderStatus = (status: GameStatus): void => {
    const forCurrentInstance = status.instanceId === instance?.id;
    if (!forCurrentInstance || status.phase === 'idle' || status.phase === 'installed') {
      stop.hidden = true;
      play.hidden = false;
      play.disabled = !bridge.isDesktop || !instance;
      playText.textContent = !instance || instance.installationState === 'installed' ? t('play') : t('installAndPlay');
      launchNote.textContent = status.lastError ?? (instance ? t('launchPending') : t('launchPending'));
      return;
    }
    if (status.phase === 'failed') {
      stop.hidden = true;
      play.hidden = false;
      play.disabled = !bridge.isDesktop;
      playText.textContent = instance?.installationState === 'installed' ? t('play') : t('installAndPlay');
      launchNote.textContent = status.lastError ?? status.message;
      return;
    }
    launchNote.textContent = status.lastError ?? status.message;
    if (status.phase === 'running') {
      play.hidden = true;
      stop.hidden = false;
      return;
    }
    stop.hidden = true;
    play.hidden = false;
    play.disabled = true;
    playText.textContent = status.phase === 'installing' ? t('installing') : t('launching');
  };
  renderStatus({ instanceId: null, phase: 'idle', message: '', completedFiles: 0, totalFiles: 0, completedBytes: 0, totalBytes: 0, pid: null, lastError: null });
  play.onclick = () => {
    if (!instance || !bridge.isDesktop || play.disabled) return;
    play.disabled = true;
    playText.textContent = instance.installationState === 'installed' ? t('launching') : t('installing');
    launchNote.textContent = instance.installationState === 'installed' ? t('launching') : t('installing');
    void launchMinecraft(instance).then(renderStatus).catch(error => {
      renderStatus({ instanceId: instance.id, phase: 'failed', message: '', completedFiles: 0, totalFiles: 0, completedBytes: 0, totalBytes: 0, pid: null, lastError: error instanceof Error ? error.message : String(error) });
      reportError(error);
    });
  };
  play.setAttribute('aria-describedby', 'launchNote');
  const quickActions = page.querySelector('.instance-quick-actions')!;
  if (instance) quickActions.append(button(t('folder'), () => bridge.openFolder('instance', instance.id)),
    button(t('install'), () => installMinecraft(instance)),
    button(t('quickSettings'), () => instanceForm(instance)));
  else quickActions.append(button(t('create'), () => instanceForm(), 'primary-action'));
  page.querySelector('.play-controls')!.append(stop);
  if (bridge.isDesktop) {
    const refreshStatus = (): void => {
      void bridge.minecraftStatus().then(status => {
        if (page.isConnected) renderStatus(status);
      }).catch(() => undefined).finally(() => {
        if (page.isConnected) window.setTimeout(refreshStatus, 1500);
      });
    };
    window.setTimeout(refreshStatus, 500);
  }
  page.querySelectorAll<HTMLButtonElement>('[data-route]').forEach(node => {
    node.onclick = () => navigate(node.dataset.route as Route);
  });
  const library = page.querySelector('#homeLibrary')!;
  if (!store.get().instances.length) library.append(el('p', 'empty-state', t('noLibrary')));
  else for (const item of store.get().instances.slice(0, 3)) {
    const row = el('article', 'collection');
    row.append(icon('instances'), el('strong', '', item.name),
      el('small', '', `${item.loader} ${item.minecraftVersion}`),
      button(t('choose'), async () => { store.set(await bridge.selectInstance(item.id)); }));
    library.append(row);
  }
  page.querySelector<HTMLButtonElement>('#officialNews')!.onclick = () => {
    void bridge.openExternal('https://www.minecraft.net/en-us/articles').catch(error => {
      page.querySelector('#newsState')!.textContent = String(error);
    });
  };
  return page;
}
