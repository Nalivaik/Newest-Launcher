# GitHub Actions Setup - Результат выполнения

**Дата:** 19 сентября 2026  
**Задача:** Настройка CI/CD для сборки релизных бинарников

---

## ✅ Выполнено

### Созданные/изменённые файлы

1. **`.github/workflows/build.yml`** (новый) - основной multi-platform workflow
2. **`.github/workflows/README.md`** (новый) - краткая документация workflows
3. **`docs/CI_CD_BUILD_GUIDE.md`** (новый) - полная документация CI/CD
4. **`BUILD_INSTRUCTIONS.md`** (обновлён) - добавлена ссылка на CI/CD guide
5. **`.github/workflows/build-windows.yml`** (удалён) - заменён на универсальный workflow

---

## 🎯 Что делает workflow

### Платформы сборки

| Платформа | Runner | Rust Target | Артефакты |
|-----------|--------|-------------|-----------|
| **Linux x86_64** | `ubuntu-latest` | `x86_64-unknown-linux-gnu` | `.deb`, `.AppImage` |
| **Linux ARM64** | `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` | `.deb`, `.AppImage` |
| **Windows x86_64** | `windows-latest` | `x86_64-pc-windows-msvc` | `.exe`, `.msi` |

### Этапы сборки

Для каждой платформы:

1. ✅ Checkout репозитория
2. ✅ Setup Node.js 22 (с npm cache)
3. ✅ Setup Rust stable (с target из matrix)
4. ✅ Установка системных зависимостей (Linux only):
   - `build-essential`
   - `pkg-config`
   - `libdbus-1-dev`
   - `libgtk-3-dev`
   - `libwebkit2gtk-4.1-dev`
   - `libssl-dev`
   - `libayatana-appindicator3-dev`
   - `librsvg2-dev`
5. ✅ `npm ci` - установка Node зависимостей
6. ✅ `npm run typecheck` - TypeScript проверка
7. ✅ `cargo check --workspace` - Rust проверка
8. ✅ `npm run desktop:build` - сборка Tauri release
9. ✅ Верификация созданных артефактов
10. ✅ Upload артефактов в GitHub Actions

---

## 🚀 Как запустить сборку

### Через GitHub Web Interface

1. Перейдите в ваш GitHub репозиторий
2. Откройте вкладку **Actions**
3. В списке workflows слева выберите **"Build Newest Launcher"**
4. Справа вверху нажмите кнопку **"Run workflow"**
5. В выпадающем меню:
   - Выберите ветку (обычно `main` или `master`)
   - Нажмите зелёную кнопку **"Run workflow"**

Workflow запустится, и вы увидите прогресс трёх параллельных сборок.

### Примерное время выполнения

- **Linux x86_64**: ~8-12 минут
- **Linux ARM64**: ~10-15 минут
- **Windows x86_64**: ~12-18 минут

**Итого:** ~10-20 минут (сборки параллельные)

---

## 📦 Где скачать артефакты

После завершения workflow:

1. Откройте страницу завершённого workflow run
2. Прокрутите вниз до секции **"Artifacts"**
3. Вы увидите следующие zip-архивы:

### Linux x86_64 (amd64)

- **`Newest-Launcher-linux-x86_64-deb`**
  - Содержит: `Newest Launcher_0.1.0_amd64.deb`
  - Размер: ~7 MB
  
- **`Newest-Launcher-linux-x86_64-AppImage`**
  - Содержит: `Newest Launcher_0.1.0_amd64.AppImage`
  - Размер: ~80 MB

### Linux ARM64 (aarch64)

- **`Newest-Launcher-linux-aarch64-deb`**
  - Содержит: `Newest Launcher_0.1.0_arm64.deb`
  
- **`Newest-Launcher-linux-aarch64-AppImage`**
  - Содержит: `Newest Launcher_0.1.0_aarch64.AppImage`

### Windows x86_64

- **`Newest-Launcher-windows-x86_64-exe`**
  - Содержит: `Newest Launcher_0.1.0_x64-setup.exe` (NSIS installer)
  
- **`Newest-Launcher-windows-x86_64-msi`** (опционально)
  - Содержит: `Newest Launcher_0.1.0_x64_en-US.msi` (MSI installer)

---

## ⚠️ Важные замечания

### ARM64 Runner

**Для Linux ARM64 сборки используется `ubuntu-24.04-arm` runner.**

- ✅ **Для платных/enterprise репозиториев**: ARM runners обычно доступны
- ⚠️ **Для бесплатных публичных репозиториев**: ARM runners могут быть недоступны

**Если ARM runner недоступен:**
- Job будет помечен как `failed` или `skipped`
- **Остальные сборки (x86_64 и Windows) всё равно выполнятся** благодаря `fail-fast: false`

**Решение для бесплатных репозиториев:**
1. Временно закомментировать ARM64 job в `build.yml`
2. Или использовать self-hosted ARM64 runner
3. Или собирать ARM64 локально на ARM-машине

### Именование файлов

Tauri автоматически добавляет версию и архитектуру в имена файлов:

- Linux: `Newest Launcher_0.1.0_amd64.deb`
- Windows: `Newest Launcher_0.1.0_x64-setup.exe`

Это стандартное поведение и его не нужно менять для корректной работы updater.

### Release процесс

**На данный момент workflow НЕ создаёт GitHub Release автоматически.**

После скачивания и тестирования артефактов:

1. Создайте GitHub Release вручную через веб-интерфейс
2. Прикрепите скачанные артефакты к Release
3. Напишите Release Notes

В будущем можно настроить автоматический Release при создании git tag.

---

## 🔍 Верификация артефактов

Workflow автоматически проверяет корректность созданных файлов перед upload:

### Linux

**DEB пакет:**
```bash
dpkg-deb --info "Newest Launcher_0.1.0_amd64.deb"
```

**AppImage:**
```bash
file "Newest Launcher_0.1.0_amd64.AppImage"
# Проверка: ELF 64-bit LSB pie executable
# Проверка: executable bit установлен
```

### Windows

**Проверка наличия .exe и .msi в bundle директориях**

Если файлы не найдены - workflow завершится с ошибкой.

---

## 📊 Matrix Strategy

Workflow использует GitHub Actions matrix для параллельной сборки:

```yaml
strategy:
  fail-fast: false  # Продолжить даже если одна платформа упала
  matrix:
    include:
      - platform: linux-x86_64
        os: ubuntu-latest
        rust-target: x86_64-unknown-linux-gnu
      
      - platform: linux-aarch64
        os: ubuntu-24.04-arm
        rust-target: aarch64-unknown-linux-gnu
      
      - platform: windows-x86_64
        os: windows-latest
        rust-target: x86_64-pc-windows-msvc
```

**`fail-fast: false`** означает:
- Если ARM64 недоступен - x86_64 и Windows всё равно соберутся
- Если Windows упал - Linux сборки завершатся успешно

---

## ✅ Что НЕ было изменено

- ✅ Код проекта не тронут (ни Frontend, ни Rust backend)
- ✅ Minecraft Core не изменён
- ✅ Loaders не изменены
- ✅ OAuth не изменён
- ✅ UI не изменён
- ✅ `tauri.conf.json` не изменён (уже был корректен)
- ✅ Никакой `cargo clean` не выполнялся

**Изменены только CI/CD конфигурация и документация.**

---

## 📖 Полная документация

Для детальной информации см.: **[docs/CI_CD_BUILD_GUIDE.md](docs/CI_CD_BUILD_GUIDE.md)**

Включает:
- Подробное описание workflow
- Troubleshooting
- Локальная сборка
- Технические детали
- Следующие шаги (code signing, auto-updater, auto-release)

---

## 🎉 Готово к использованию

Workflow полностью готов к запуску. После push в GitHub:

1. Actions → Build Newest Launcher → Run workflow
2. Дождаться завершения
3. Скачать артефакты
4. Протестировать на соответствующих платформах
5. Создать Release вручную с протестированными артефактами

**Никаких дополнительных изменений в код проекта не требуется.**
