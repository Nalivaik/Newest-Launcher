import type { Settings, StorageUsage } from '../types';
import { store } from '../state/store';
import { bridge } from '../services/bridge';
import { button, el, field, notify, reportError } from '../components/ui';

const messages = {
  ru: {
    title: 'Настройки', subtitle: 'Внешний вид, параметры новых установок и локальные данные.',
    save: 'Сохранить изменения', saving: 'Сохраняем…', saved: 'Настройки сохранены.', appearance: 'Внешний вид',
    language: 'Язык', theme: 'Тема', dark: 'Тёмная', light: 'Светлая', system: 'Как в системе',
    animations: 'Анимации интерфейса', transparency: 'Прозрачные панели', scale: 'Масштаб интерфейса',
    appearanceNote: 'Изменения интерфейса применяются после сохранения.', preferences: 'Новые установки',
    ram: 'Память по умолчанию, МБ', ramNote: 'Используется при создании установки. У существующих установок свои параметры памяти.',
    downloads: 'Загрузки', concurrency: 'Одновременные загрузки',
    downloadsNote: 'Ограничение сохранено для менеджера загрузок. Каждый файл Minecraft и Modrinth проверяется до использования.',
    storage: 'Хранилище', dataDirectory: 'Папка данных', instances: 'Установки', cache: 'Кеш', trash: 'Корзина', free: 'Свободно на диске',
    usageLoading: 'Подсчитываем размер файлов…', usageError: 'Не удалось прочитать размер файлов. Повторите проверку.',
    refresh: 'Обновить', openData: 'Открыть данные', openCache: 'Открыть кеш', openLogs: 'Открыть журналы',
    preview: 'Браузерный просмотр: сохранение настроек и работа с файлами доступны в desktop-приложении.',
    previewStorage: 'Размер файлов доступен в desktop-приложении.', reset: 'Отменить изменения',
  },
  en: {
    title: 'Settings', subtitle: 'Appearance, defaults for new instances and local data.',
    save: 'Save changes', saving: 'Saving…', saved: 'Settings saved.', appearance: 'Appearance',
    language: 'Language', theme: 'Theme', dark: 'Dark', light: 'Light', system: 'Follow system',
    animations: 'Interface animations', transparency: 'Transparent panels', scale: 'Interface scale',
    appearanceNote: 'Appearance changes apply after saving.', preferences: 'New instances',
    ram: 'Default memory, MB', ramNote: 'Used when creating an instance. Existing instances keep their own memory settings.',
    downloads: 'Downloads', concurrency: 'Concurrent downloads',
    downloadsNote: 'The limit is retained for the download manager. Every Minecraft and Modrinth file is verified before use.',
    storage: 'Storage', dataDirectory: 'Data directory', instances: 'Instances', cache: 'Cache', trash: 'Trash', free: 'Free disk space',
    usageLoading: 'Calculating file sizes…', usageError: 'Could not read file sizes. Try refreshing.',
    refresh: 'Refresh', openData: 'Open data', openCache: 'Open cache', openLogs: 'Open logs',
    preview: 'Browser preview: saving settings and working with files requires the desktop app.',
    previewStorage: 'File sizes are available in the desktop app.', reset: 'Discard changes',
  },
  uk: {
    title: 'Налаштування', subtitle: 'Зовнішній вигляд, параметри нових збірок і локальні дані.',
    save: 'Зберегти зміни', saving: 'Зберігаємо…', saved: 'Налаштування збережено.', appearance: 'Зовнішній вигляд',
    language: 'Мова', theme: 'Тема', dark: 'Темна', light: 'Світла', system: 'Як у системі',
    animations: 'Анімації інтерфейсу', transparency: 'Прозорі панелі', scale: 'Масштаб інтерфейсу',
    appearanceNote: 'Зміни інтерфейсу застосовуються після збереження.', preferences: 'Нові збірки',
    ram: 'Пам’ять за замовчуванням, МБ', ramNote: 'Використовується під час створення збірки. Існуючі збірки мають власні параметри пам’яті.',
    downloads: 'Завантаження', concurrency: 'Одночасні завантаження',
    downloadsNote: 'Обмеження збережено для менеджера завантажень. Кожен файл Minecraft і Modrinth перевіряється перед використанням.',
    storage: 'Сховище', dataDirectory: 'Папка даних', instances: 'Збірки', cache: 'Кеш', trash: 'Кошик', free: 'Вільно на диску',
    usageLoading: 'Підраховуємо розмір файлів…', usageError: 'Не вдалося прочитати розмір файлів. Повторіть перевірку.',
    refresh: 'Оновити', openData: 'Відкрити дані', openCache: 'Відкрити кеш', openLogs: 'Відкрити журнали',
    preview: 'Браузерний перегляд: збереження налаштувань і робота з файлами доступні у desktop-застосунку.',
    previewStorage: 'Розмір файлів доступний у desktop-застосунку.', reset: 'Скасувати зміни',
  },
};

function selectControl<T extends string>(options: ReadonlyArray<readonly [T, string]>, value: T): HTMLSelectElement {
  const select = el('select');
  for (const [key, label] of options) {
    const option = el('option', '', label);
    option.value = key;
    select.append(option);
  }
  select.value = value;
  return select;
}

function checkbox(checked: boolean): HTMLInputElement {
  const input = el('input');
  input.type = 'checkbox';
  input.checked = checked;
  return input;
}

function bytes(value: number, locale: string): string {
  if (!Number.isFinite(value) || value < 0) return '—';
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  const exponent = value === 0 ? 0 : Math.min(Math.floor(Math.log(value) / Math.log(1024)), units.length - 1);
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(value / 1024 ** exponent)} ${units[exponent]}`;
}

export function settingsPage(): HTMLElement {
  const snapshot = store.get();
  const copy = messages[snapshot.settings.language];
  const settings = snapshot.settings;
  const page = el('section', 'settings-page');
  const form = el('form', 'settings-form');
  const header = el('div', 'library-head');
  const heading = el('div');
  heading.append(el('h1', '', copy.title), el('p', '', copy.subtitle));
  const save = el('button', 'primary-action', copy.save);
  save.type = 'submit';
  save.disabled = !bridge.isDesktop;
  if (!bridge.isDesktop) save.title = copy.preview;
  header.append(heading, save);
  form.append(header);
  if (!bridge.isDesktop) form.append(el('p', 'notice', copy.preview));

  const appearance = el('section', 'panel setting-group');
  appearance.append(el('h2', '', copy.appearance));
  const appearanceGrid = el('div', 'form-grid');
  const language = selectControl<Settings['language']>([['ru', 'Русский'], ['en', 'English'], ['uk', 'Українська']], settings.language);
  const theme = selectControl<Settings['theme']>([['dark', copy.dark], ['light', copy.light], ['system', copy.system]], settings.theme);
  const animations = checkbox(settings.animations);
  const transparency = checkbox(settings.transparency);
  const scale = el('input');
  scale.type = 'range';
  scale.min = '0.8';
  scale.max = '1.25';
  scale.step = '0.05';
  scale.value = String(settings.uiScale);
  const scaleOutput = el('output', 'scale-value', `${Math.round(settings.uiScale * 100)}%`);
  scale.addEventListener('input', () => { scaleOutput.value = `${Math.round(scale.valueAsNumber * 100)}%`; });
  const scaleField = field(copy.scale, scale);
  scaleField.append(scaleOutput);
  appearanceGrid.append(field(copy.language, language), field(copy.theme, theme), field(copy.animations, animations), field(copy.transparency, transparency), scaleField);
  appearance.append(appearanceGrid, el('p', 'field-hint', copy.appearanceNote));

  const preferences = el('section', 'panel setting-group');
  const ram = el('input');
  ram.type = 'number';
  ram.min = '512';
  ram.max = '65536';
  ram.step = '256';
  ram.required = true;
  ram.value = String(settings.defaultRamMb);
  preferences.append(el('h2', '', copy.preferences), field(copy.ram, ram), el('p', 'field-hint', copy.ramNote));
  const downloads = el('section', 'panel setting-group');
  const concurrency = el('input');
  concurrency.type = 'number';
  concurrency.min = '1';
  concurrency.max = '16';
  concurrency.step = '1';
  concurrency.required = true;
  concurrency.value = String(settings.concurrentDownloads);
  downloads.append(el('h2', '', copy.downloads), field(copy.concurrency, concurrency), el('p', 'field-hint', copy.downloadsNote));
  form.append(appearance, preferences, downloads);
  const footer = el('div', 'dialog-actions');
  const reset = button(copy.reset, () => {
    language.value = settings.language;
    theme.value = settings.theme;
    animations.checked = settings.animations;
    transparency.checked = settings.transparency;
    scale.value = String(settings.uiScale);
    scaleOutput.value = `${Math.round(settings.uiScale * 100)}%`;
    ram.value = String(settings.defaultRamMb);
    concurrency.value = String(settings.concurrentDownloads);
  }, 'soft-button');
  footer.append(reset);
  form.append(footer);
  form.addEventListener('submit', event => {
    event.preventDefault();
    if (!bridge.isDesktop || save.disabled || !form.reportValidity()) return;
    const updated: Settings = {
      language: language.value as Settings['language'], theme: theme.value as Settings['theme'],
      animations: animations.checked, transparency: transparency.checked,
      uiScale: scale.valueAsNumber, defaultRamMb: ram.valueAsNumber, concurrentDownloads: concurrency.valueAsNumber,
    };
    save.disabled = true;
    reset.disabled = true;
    save.textContent = copy.saving;
    form.setAttribute('aria-busy', 'true');
    void (async () => {
      try {
        const next = await bridge.saveSettings(updated);
        if (next) store.set(next);
        notify(messages[updated.language].saved);
      } catch (error: unknown) {
        reportError(error);
      } finally {
        save.disabled = false;
        reset.disabled = false;
        save.textContent = copy.save;
        form.removeAttribute('aria-busy');
      }
    })();
  });
  page.append(form);

  const storage = el('section', 'panel setting-group storage-panel');
  const storageHeader = el('div', 'panel-head');
  storageHeader.append(el('h2', '', copy.storage));
  const status = el('p', 'field-hint', bridge.isDesktop ? copy.usageLoading : copy.previewStorage);
  status.setAttribute('role', 'status');
  const sizes = el('dl', 'storage-usage');
  const values = new Map<keyof StorageUsage, HTMLElement>();
  const rows: ReadonlyArray<readonly [keyof StorageUsage, string]> = [
    ['instancesBytes', copy.instances], ['cacheBytes', copy.cache], ['trashBytes', copy.trash], ['freeBytes', copy.free],
  ];
  for (const [key, label] of rows) {
    const value = el('dd', '', '—');
    values.set(key, value);
    sizes.append(el('dt', '', label), value);
  }
  const refreshStorage = async (): Promise<void> => {
    status.textContent = copy.usageLoading;
    storage.setAttribute('aria-busy', 'true');
    try {
      const usage = await bridge.storageUsage();
      if (!storage.isConnected) return;
      for (const [key, value] of values) value.textContent = bytes(usage[key], settings.language);
      status.textContent = '';
    } catch (error: unknown) {
      if (storage.isConnected) status.textContent = copy.usageError;
      throw error;
    } finally {
      storage.removeAttribute('aria-busy');
    }
  };
  const refresh = button(copy.refresh, refreshStorage, 'soft-button');
  refresh.disabled = !bridge.isDesktop;
  storageHeader.append(refresh);
  const path = el('p', 'storage-path');
  path.append(el('span', '', copy.dataDirectory), el('code', '', snapshot.dataDirectory || copy.previewStorage));
  const folders = el('div', 'section-actions');
  for (const [kind, label] of [['data', copy.openData], ['cache', copy.openCache], ['logs', copy.openLogs]]) {
    const open = button(label, () => bridge.openFolder(kind), 'soft-button');
    open.disabled = !bridge.isDesktop;
    if (!bridge.isDesktop) open.title = copy.preview;
    folders.append(open);
  }
  storage.append(storageHeader, path, sizes, status, folders);
  page.append(storage);
  if (bridge.isDesktop) void refreshStorage().catch(reportError);
  return page;
}
