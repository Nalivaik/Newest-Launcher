import type { Instance, InstanceInput, Loader } from '../types';
import { store } from '../state/store';
import { bridge } from '../services/bridge';
import { gameVersions } from '../services/catalog';
import { button, el, field, notify, reportError, showDialog } from './ui';

const messages = {
  ru: {
    create: 'Новая установка', edit: 'Настройки установки', name: 'Название', version: 'Версия Minecraft',
    loader: 'Загрузчик', loaderVersion: 'Версия загрузчика', loaderAutomatic: 'Будет выбрана при установке',
    namePlaceholder: 'Мой Minecraft', manual: 'Версию можно ввести вручную. Доступность загрузчика для неё проверяется отдельно при установке.',
    loading: 'Получаем подсказки версий…', loaded: 'Версии загружены. Начните ввод, чтобы сузить список.',
    offline: 'Список версий недоступен. Введите нужную версию вручную.',
    launch: 'Параметры запуска', launchNote: 'Для Vanilla эти параметры используются реальным desktop-установщиком и Java-процессом. Поддержка загрузчиков добавляется отдельным этапом.',
    java: 'Путь к Java (необязательно)', javaPlaceholder: 'Полный путь к java / javaw', ram: 'Оперативная память, МБ',
    width: 'Ширина окна', height: 'Высота окна', fullscreen: 'Полноэкранный режим', jvm: 'Аргументы JVM — по одному аргументу на строку',
    cancel: 'Отмена', save: 'Сохранить', createAction: 'Создать установку', saving: 'Сохраняем…', saved: 'Настройки установки сохранены.', created: 'Установка создана.',
    preview: 'Создание и изменение установок доступны в desktop-приложении.', versionRequired: 'Укажите версию Minecraft.',
  },
  en: {
    create: 'New instance', edit: 'Instance settings', name: 'Name', version: 'Minecraft version',
    loader: 'Loader', loaderVersion: 'Loader version', loaderAutomatic: 'Selected during installation',
    namePlaceholder: 'My Minecraft', manual: 'You can enter a version manually. Loader support is checked separately during installation.',
    loading: 'Loading version suggestions…', loaded: 'Versions loaded. Start typing to narrow the list.',
    offline: 'Version suggestions are unavailable. Enter a version manually.',
    launch: 'Launch preferences', launchNote: 'For Vanilla, these values are used by the real desktop installer and Java process. Loader support is added in a separate milestone.',
    java: 'Java executable path (optional)', javaPlaceholder: 'Full path to java / javaw', ram: 'Memory, MB',
    width: 'Window width', height: 'Window height', fullscreen: 'Fullscreen', jvm: 'JVM arguments — one argument per line',
    cancel: 'Cancel', save: 'Save', createAction: 'Create instance', saving: 'Saving…', saved: 'Instance settings saved.', created: 'Instance created.',
    preview: 'Creating and editing instances requires the desktop app.', versionRequired: 'Enter a Minecraft version.',
  },
  uk: {
    create: 'Нова збірка', edit: 'Налаштування збірки', name: 'Назва', version: 'Версія Minecraft',
    loader: 'Завантажувач', loaderVersion: 'Версія завантажувача', loaderAutomatic: 'Буде обрано під час встановлення',
    namePlaceholder: 'Мій Minecraft', manual: 'Версію можна ввести вручну. Підтримка завантажувача перевіряється окремо під час встановлення.',
    loading: 'Отримуємо підказки версій…', loaded: 'Версії завантажено. Почніть вводити, щоб звузити список.',
    offline: 'Список версій недоступний. Введіть потрібну версію вручну.',
    launch: 'Параметри запуску', launchNote: 'Для Vanilla ці параметри використовує реальний desktop-встановлювач і Java-процес. Підтримку завантажувачів буде додано окремим етапом.',
    java: 'Шлях до Java (необов’язково)', javaPlaceholder: 'Повний шлях до java / javaw', ram: 'Оперативна пам’ять, МБ',
    width: 'Ширина вікна', height: 'Висота вікна', fullscreen: 'Повноекранний режим', jvm: 'Аргументи JVM — по одному аргументу на рядок',
    cancel: 'Скасувати', save: 'Зберегти', createAction: 'Створити збірку', saving: 'Зберігаємо…', saved: 'Налаштування збірки збережено.', created: 'Збірку створено.',
    preview: 'Створення та редагування збірок доступні у desktop-застосунку.', versionRequired: 'Вкажіть версію Minecraft.',
  },
};

const loaders: ReadonlyArray<{ value: Loader; label: string }> = [
  { value: 'vanilla', label: 'Vanilla' }, { value: 'fabric', label: 'Fabric' },
  { value: 'forge', label: 'Forge' }, { value: 'neoforge', label: 'NeoForge' },
  { value: 'quilt', label: 'Quilt' },
];
const MAX_VERSION_SUGGESTIONS = 200;

function numberInput(value: number, min: number, max: number, step = 1): HTMLInputElement {
  const input = el('input');
  input.type = 'number';
  input.value = String(value);
  input.min = String(min);
  input.max = String(max);
  input.step = String(step);
  input.required = true;
  return input;
}

export async function instanceForm(instance?: Instance): Promise<void> {
  const snapshot = store.get();
  const copy = messages[snapshot.settings.language];
  const form = el('form', 'instance-form');
  const grid = el('div', 'form-grid');

  const name = el('input');
  name.value = instance?.name ?? '';
  name.placeholder = copy.namePlaceholder;
  name.required = true;
  name.maxLength = 80;
  name.autocomplete = 'off';

  const version = el('input');
  version.value = instance?.minecraftVersion ?? '';
  version.placeholder = '1.21.1';
  version.required = true;
  version.maxLength = 64;
  version.autocomplete = 'off';
  version.spellcheck = false;
  const suggestions = el('datalist');
  suggestions.id = `minecraft-versions-${crypto.randomUUID()}`;
  version.setAttribute('list', suggestions.id);
  const versionStatus = el('p', 'field-hint', copy.loading);
  versionStatus.setAttribute('role', 'status');
  const versionField = field(copy.version, version);
  versionField.append(suggestions, versionStatus);

  const loader = el('select');
  for (const item of loaders) {
    const option = el('option', '', item.label);
    option.value = item.value;
    loader.append(option);
  }
  loader.value = instance?.loader ?? 'vanilla';
  const loaderVersion = el('input');
  loaderVersion.value = instance?.loaderVersion ?? '';
  loaderVersion.placeholder = copy.loaderAutomatic;
  loaderVersion.maxLength = 64;
  loaderVersion.autocomplete = 'off';
  const syncLoader = (): void => { loaderVersion.disabled = loader.value === 'vanilla'; };
  loader.addEventListener('change', syncLoader);
  syncLoader();
  grid.append(field(copy.name, name), versionField, field(copy.loader, loader), field(copy.loaderVersion, loaderVersion));
  form.append(grid, el('p', 'field-hint', copy.manual));

  const preferences = el('details', 'launch-preferences');
  preferences.append(el('summary', '', copy.launch), el('p', 'notice', copy.launchNote));
  const launchGrid = el('div', 'form-grid');
  const java = el('input');
  java.value = instance?.javaPath ?? '';
  java.placeholder = copy.javaPlaceholder;
  java.maxLength = 4096;
  java.spellcheck = false;
  const ram = numberInput(instance?.ramMb ?? snapshot.settings.defaultRamMb, 512, 65536, 256);
  const width = numberInput(instance?.resolution.width ?? 1280, 320, 16384);
  const height = numberInput(instance?.resolution.height ?? 720, 240, 16384);
  const fullscreen = el('input');
  fullscreen.type = 'checkbox';
  fullscreen.checked = instance?.resolution.fullscreen ?? false;
  const args = el('textarea');
  args.value = instance?.jvmArgs.join('\n') ?? '';
  args.rows = 3;
  args.maxLength = 32768;
  args.placeholder = '-XX:+UseG1GC';
  args.spellcheck = false;
  launchGrid.append(field(copy.java, java), field(copy.ram, ram), field(copy.width, width), field(copy.height, height));
  preferences.append(launchGrid, field(copy.fullscreen, fullscreen), field(copy.jvm, args));
  form.append(preferences);

  const footer = el('div', 'dialog-actions');
  const submit = el('button', 'primary-action', instance ? copy.save : copy.createAction);
  submit.type = 'submit';
  submit.disabled = !bridge.isDesktop;
  if (!bridge.isDesktop) form.append(el('p', 'notice', copy.preview));
  const modal = showDialog(instance ? copy.edit : copy.create, form);
  const cancel = button(copy.cancel, () => modal.close(), 'soft-button');
  footer.append(cancel, submit);
  form.append(footer);

  let availableVersions: Awaited<ReturnType<typeof gameVersions>> = [];
  const renderSuggestions = (): void => {
    const query = version.value.trim().toLowerCase();
    const matching = availableVersions.filter(item => item.version.toLowerCase().includes(query)).slice(0, MAX_VERSION_SUGGESTIONS);
    suggestions.replaceChildren(...matching.map(item => {
      const option = el('option');
      option.value = item.version;
      option.label = item.version_type;
      return option;
    }));
  };
  version.addEventListener('input', () => { version.setCustomValidity(''); renderSuggestions(); });
  void gameVersions().then(versions => {
    if (!form.isConnected) return;
    availableVersions = versions;
    versionStatus.textContent = copy.loaded;
    renderSuggestions();
  }).catch(() => {
    if (form.isConnected) versionStatus.textContent = copy.offline;
  });

  form.addEventListener('submit', event => {
    event.preventDefault();
    if (!bridge.isDesktop || submit.disabled) return;
    if (!version.value.trim()) {
      version.setCustomValidity(copy.versionRequired);
      version.reportValidity();
      return;
    }
    if (!form.reportValidity()) return;
    const input: InstanceInput = {
      name: name.value.trim(), minecraftVersion: version.value.trim(), loader: loader.value as Loader,
      loaderVersion: loader.value === 'vanilla' ? null : loaderVersion.value.trim() || null,
      javaPath: java.value.trim() || null, ramMb: ram.valueAsNumber,
      jvmArgs: args.value.split(/\r?\n/).map(argument => argument.trim()).filter(Boolean),
      resolution: { width: width.valueAsNumber, height: height.valueAsNumber, fullscreen: fullscreen.checked },
    };
    submit.disabled = true;
    cancel.disabled = true;
    submit.textContent = copy.saving;
    form.setAttribute('aria-busy', 'true');
    void (async () => {
      try {
        const next = instance ? await bridge.updateInstance(instance.id, input) : await bridge.createInstance(input);
        if (next) store.set(next);
        modal.close();
        notify(instance ? copy.saved : copy.created);
      } catch (error: unknown) {
        reportError(error);
      } finally {
        submit.disabled = false;
        cancel.disabled = false;
        submit.textContent = instance ? copy.save : copy.createAction;
        form.removeAttribute('aria-busy');
      }
    })();
  });
  name.focus();
}
