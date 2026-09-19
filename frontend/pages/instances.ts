import type { Instance, Language } from '../types';
import { store } from '../state/store';
import { bridge } from '../services/bridge';
import { instanceForm } from '../components/instance-form';
import { installMinecraft, launchMinecraft } from '../components/minecraft-actions';
import { button, confirmDialog, el, field, notify, reportError, showDialog } from '../components/ui';
import { t } from '../i18n';

const messages = {
  ru: {
    title: 'Твои установки', subtitle: 'Отдельная папка, версия и настройки для каждого мира Minecraft.',
    create: 'Новая установка', import: 'Импортировать', importDone: 'Установка импортирована.',
    search: 'Найти по названию, версии или загрузчику', emptyTitle: 'Здесь начинается твой Minecraft',
    empty: 'Создай первую установку: выбери версию и загрузчик, задай память и параметры окна.',
    emptySearch: 'Установки не найдены', active: 'Активная', select: 'Выбрать', selected: 'Активная установка изменена.',
    notInstalled: 'Minecraft ещё не установлен', configure: 'Настроить', actions: 'Действия', folder: 'Открыть папку',
    clone: 'Клонировать', cloneTitle: 'Клонировать установку', cloneName: 'Название копии', copySuffix: 'копия',
    cloneNote: 'Создаётся отдельная копия установки и её файлов. Для больших установок это может занять время.',
    cloned: 'Копия установки создана.', export: 'Экспортировать', exported: 'Установка экспортирована.',
    remove: 'В корзину', removeTitle: 'Переместить установку в корзину?',
    removeBody: 'Файлы сохранятся в корзине лаунчера. Установку можно будет восстановить.', removed: 'Установка перемещена в корзину.',
    trash: 'Корзина установок', trashNote: 'Файлы в корзине остаются на диске. Восстановление возвращает установку в библиотеку.',
    restore: 'Восстановить', restored: 'Установка восстановлена.', created: 'Создана', lastPlayed: 'Последний запуск',
    never: 'Ещё не запускалась', playtime: 'Время игры', minutes: 'мин', hours: 'ч', ram: 'Память',
    cancel: 'Отмена', saving: 'Копирование…',
    preview: 'Вы открыли браузерный просмотр. Управление локальными установками доступно в desktop-приложении.',
  },
  en: {
    title: 'Your instances', subtitle: 'A separate folder, version and settings for each Minecraft adventure.',
    create: 'New instance', import: 'Import', importDone: 'Instance imported.',
    search: 'Search by name, version or loader', emptyTitle: 'Your Minecraft starts here',
    empty: 'Create your first instance: choose a version and loader, then set memory and window preferences.',
    emptySearch: 'No matching instances', active: 'Active', select: 'Select', selected: 'Active instance changed.',
    notInstalled: 'Minecraft is not installed yet', configure: 'Configure', actions: 'Actions', folder: 'Open folder',
    clone: 'Clone', cloneTitle: 'Clone instance', cloneName: 'Copy name', copySuffix: 'copy',
    cloneNote: 'Creates an independent copy of this instance and its files. Large instances may take a while.',
    cloned: 'Instance cloned.', export: 'Export', exported: 'Instance exported.',
    remove: 'Move to trash', removeTitle: 'Move this instance to trash?',
    removeBody: 'Files remain in the launcher trash. You can restore the instance later.', removed: 'Instance moved to trash.',
    trash: 'Instance trash', trashNote: 'Trashed files remain on disk. Restoring an instance returns it to your library.',
    restore: 'Restore', restored: 'Instance restored.', created: 'Created', lastPlayed: 'Last played',
    never: 'Not played yet', playtime: 'Playtime', minutes: 'min', hours: 'h', ram: 'Memory',
    cancel: 'Cancel', saving: 'Copying…',
    preview: 'You are viewing the browser preview. Local instance management requires the desktop app.',
  },
  uk: {
    title: 'Твої збірки', subtitle: 'Окрема папка, версія та налаштування для кожної пригоди Minecraft.',
    create: 'Нова збірка', import: 'Імпортувати', importDone: 'Збірку імпортовано.',
    search: 'Знайти за назвою, версією або завантажувачем', emptyTitle: 'Тут починається твій Minecraft',
    empty: 'Створи першу збірку: обери версію та завантажувач, задай пам’ять і параметри вікна.',
    emptySearch: 'Збірок не знайдено', active: 'Активна', select: 'Обрати', selected: 'Активну збірку змінено.',
    notInstalled: 'Minecraft ще не встановлено', configure: 'Налаштувати', actions: 'Дії', folder: 'Відкрити папку',
    clone: 'Клонувати', cloneTitle: 'Клонувати збірку', cloneName: 'Назва копії', copySuffix: 'копія',
    cloneNote: 'Створюється окрема копія збірки та її файлів. Для великих збірок це може зайняти час.',
    cloned: 'Копію збірки створено.', export: 'Експортувати', exported: 'Збірку експортовано.',
    remove: 'До кошика', removeTitle: 'Перемістити збірку до кошика?',
    removeBody: 'Файли залишаться у кошику лаунчера. Збірку можна буде відновити.', removed: 'Збірку переміщено до кошика.',
    trash: 'Кошик збірок', trashNote: 'Файли у кошику залишаються на диску. Відновлення повертає збірку до бібліотеки.',
    restore: 'Відновити', restored: 'Збірку відновлено.', created: 'Створена', lastPlayed: 'Останній запуск',
    never: 'Ще не запускалася', playtime: 'Ігровий час', minutes: 'хв', hours: 'год', ram: 'Пам’ять',
    cancel: 'Скасувати', saving: 'Копіювання…',
    preview: 'Відкрито браузерний перегляд. Керування локальними збірками доступне у desktop-застосунку.',
  },
};
type Copy = typeof messages.ru;
const loaderNames = { vanilla: 'Vanilla', fabric: 'Fabric', forge: 'Forge', neoforge: 'NeoForge', quilt: 'Quilt' };

function desktopAction(label: string, action: () => void | Promise<void>, className = 'soft-button'): HTMLButtonElement {
  const control = button(label, action, className);
  control.disabled = !bridge.isDesktop;
  if (!bridge.isDesktop) control.title = messages[store.get().settings.language].preview;
  return control;
}

function dateLabel(value: string | null, language: Language, copy: Copy): string {
  if (!value) return copy.never;
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? copy.never : new Intl.DateTimeFormat(language, { dateStyle: 'medium' }).format(date);
}

function playtime(instance: Instance, copy: Copy): string {
  const totalMinutes = Math.floor(instance.totalPlaytimeSeconds / 60);
  const hours = Math.floor(totalMinutes / 60);
  return hours > 0 ? `${hours} ${copy.hours} ${totalMinutes % 60} ${copy.minutes}` : `${totalMinutes} ${copy.minutes}`;
}

function cloneDialog(instance: Instance, copy: Copy): void {
  const form = el('form', 'instance-form');
  const name = el('input');
  name.required = true;
  name.maxLength = 80;
  name.value = `${instance.name.slice(0, 65)} — ${copy.copySuffix}`;
  form.append(el('p', 'dialog-note', copy.cloneNote), field(copy.cloneName, name));
  const footer = el('div', 'dialog-actions');
  const submit = el('button', 'primary-action', copy.clone);
  submit.type = 'submit';
  const modal = showDialog(copy.cloneTitle, form);
  const cancel = button(copy.cancel, () => modal.close(), 'soft-button');
  footer.append(cancel, submit);
  form.append(footer);
  form.addEventListener('submit', event => {
    event.preventDefault();
    if (submit.disabled || !form.reportValidity()) return;
    submit.disabled = true;
    cancel.disabled = true;
    submit.textContent = copy.saving;
    form.setAttribute('aria-busy', 'true');
    void (async () => {
      try {
        const next = await bridge.cloneInstance(instance.id, name.value.trim());
        if (next) store.set(next);
        modal.close();
        notify(copy.cloned);
      } catch (error: unknown) {
        reportError(error);
      } finally {
        submit.disabled = false;
        cancel.disabled = false;
        submit.textContent = copy.clone;
        form.removeAttribute('aria-busy');
      }
    })();
  });
  name.focus();
  name.select();
}

function instanceCard(instance: Instance, copy: Copy, language: Language): HTMLElement {
  const active = store.get().activeInstanceId === instance.id;
  const card = el('article', `install-card instance-card${active ? ' is-active' : ''}`);
  const top = el('div', 'install-top');
  const badge = el('span', `loader ${instance.loader}`, loaderNames[instance.loader].slice(0, 1));
  badge.setAttribute('aria-hidden', 'true');
  const title = el('div', 'instance-title');
  title.append(el('h3', '', instance.name), el('small', '', `${loaderNames[instance.loader]} ${instance.minecraftVersion}${instance.loaderVersion ? ` · ${instance.loaderVersion}` : ''}`));
  top.append(badge, title);
  if (active) top.append(el('span', 'tag active-tag', copy.active));
  const installationState = instance.installationState === 'installed' ? t('installed')
    : instance.installationState === 'corrupted' ? t('corrupted') : copy.notInstalled;
  card.append(top, el('p', 'installation-status', installationState));
  const metadata = el('dl', 'instance-meta');
  const pairs = [
    [copy.ram, `${Math.round(instance.ramMb / 1024 * 10) / 10} GiB`],
    [copy.created, dateLabel(instance.createdAt, language, copy)],
    [copy.lastPlayed, dateLabel(instance.lastPlayed, language, copy)],
    [copy.playtime, playtime(instance, copy)],
  ];
  for (const [label, value] of pairs) metadata.append(el('dt', '', label), el('dd', '', value));
  card.append(metadata);
  const actions = el('div', 'card-actions');
  const select = desktopAction(active ? copy.active : copy.select, async () => {
    const next = await bridge.selectInstance(instance.id);
    if (next) store.set(next);
    notify(copy.selected);
  }, active ? 'soft-button selected-button' : 'primary-action');
  select.disabled = active || !bridge.isDesktop;
  const installOrLaunch = instance.installationState === 'installed'
    ? desktopAction(t('play'), async () => { await launchMinecraft(instance); }, 'primary-action')
    : desktopAction(t('install'), () => installMinecraft(instance), 'primary-action');
  actions.append(select, installOrLaunch, desktopAction(copy.configure, () => instanceForm(instance)));
  const menu = el('details', 'action-menu');
  menu.append(el('summary', 'soft-button', copy.actions));
  const menuItems = el('div', 'action-menu-items');
  menuItems.append(
    desktopAction(copy.folder, () => bridge.openFolder('instance', instance.id)),
    desktopAction(copy.clone, () => cloneDialog(instance, copy)),
    desktopAction(copy.export, async () => { if (await bridge.exportInstance(instance.id)) notify(copy.exported); }),
    desktopAction(copy.remove, async () => {
      if (!await confirmDialog(copy.removeTitle, `${instance.name}\n\n${copy.removeBody}`)) return;
      const next = await bridge.deleteInstance(instance.id);
      if (next) store.set(next);
      notify(copy.removed);
    }, 'soft-button danger'),
  );
  menu.append(menuItems);
  actions.append(menu);
  card.append(actions);
  return card;
}

export function instancesPage(): HTMLElement {
  const snapshot = store.get();
  const copy = messages[snapshot.settings.language];
  const page = el('section', 'instances-page');
  const header = el('div', 'library-head');
  const heading = el('div');
  heading.append(el('h1', '', copy.title), el('p', '', copy.subtitle));
  const actions = el('div', 'section-actions');
  actions.append(
    desktopAction(copy.import, async () => {
      const next = await bridge.importInstance();
      if (next) { store.set(next); notify(copy.importDone); }
    }),
    desktopAction(copy.create, () => instanceForm(), 'primary-action'),
  );
  header.append(heading, actions);
  page.append(header);
  if (!bridge.isDesktop) page.append(el('p', 'notice', copy.preview));

  if (snapshot.instances.length === 0) {
    const empty = el('div', 'panel empty-state');
    empty.append(el('div', 'cube mini-cube'), el('h2', '', copy.emptyTitle), el('p', '', copy.empty));
    empty.append(desktopAction(copy.create, () => instanceForm(), 'primary-action'));
    page.append(empty);
  } else {
    const toolbar = el('div', 'library-toolbar');
    const search = el('input');
    search.type = 'search';
    search.placeholder = copy.search;
    search.setAttribute('aria-label', copy.search);
    const searchLabel = el('label', 'search');
    searchLabel.append(search);
    toolbar.append(searchLabel);
    const grid = el('div', 'install-grid');
    const noResults = el('p', 'panel empty-state', copy.emptySearch);
    noResults.hidden = true;
    const cards = snapshot.instances.map(instance => ({
      card: instanceCard(instance, copy, snapshot.settings.language),
      keywords: `${instance.name} ${instance.minecraftVersion} ${loaderNames[instance.loader]}`.toLocaleLowerCase(),
    }));
    grid.append(...cards.map(item => item.card));
    search.addEventListener('input', () => {
      const query = search.value.trim().toLocaleLowerCase();
      let visible = 0;
      for (const item of cards) { item.card.hidden = !item.keywords.includes(query); if (!item.card.hidden) visible += 1; }
      noResults.hidden = visible !== 0;
    });
    page.append(toolbar, grid, noResults);
  }

  if (snapshot.trash.length > 0) {
    const trash = el('details', 'panel instance-trash');
    trash.append(el('summary', '', `${copy.trash} (${snapshot.trash.length})`), el('p', 'field-hint', copy.trashNote));
    const list = el('div', 'trash-list');
    for (const instance of snapshot.trash) {
      const row = el('div', 'trash-row');
      const description = el('div');
      description.append(el('strong', '', instance.name), el('small', '', `${loaderNames[instance.loader]} ${instance.minecraftVersion}`));
      row.append(description, desktopAction(copy.restore, async () => {
        const next = await bridge.restoreInstance(instance.id);
        if (next) store.set(next);
        notify(copy.restored);
      }));
      list.append(row);
    }
    trash.append(list);
    page.append(trash);
  }
  return page;
}
