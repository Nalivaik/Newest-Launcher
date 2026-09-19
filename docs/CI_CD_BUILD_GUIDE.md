# CI/CD Build Guide - Newest Launcher

## GitHub Actions Workflow

### Описание

Проект использует GitHub Actions для автоматизированной сборки релизных бинарников на трёх платформах:

1. **Linux x86_64** (amd64)
2. **Linux ARM64** (aarch64)
3. **Windows x86_64**

### Запуск вручную (Manual Dispatch)

Workflow настроен на **ручной запуск** и не выполняется автоматически на каждый push/PR.

#### Как запустить сборку:

1. Перейдите в ваш GitHub репозиторий
2. Откройте вкладку **Actions**
3. Выберите workflow **"Build Newest Launcher"** в списке слева
4. Нажмите кнопку **"Run workflow"** справа
5. Выберите ветку для сборки (обычно `main` или `master`)
6. Нажмите зелёную кнопку **"Run workflow"**

Workflow запустится и начнёт параллельную сборку на всех трёх платформах.

### Что происходит во время сборки

Для каждой платформы:

1. ✅ Checkout репозитория
2. ✅ Установка Node.js 22 с npm cache
3. ✅ Установка Rust stable toolchain с соответствующим target
4. ✅ Установка системных зависимостей (только на Linux)
5. ✅ Установка npm зависимостей (`npm ci`)
6. ✅ TypeScript проверка (`npm run typecheck`)
7. ✅ Cargo проверка workspace (`cargo check --workspace`)
8. ✅ Сборка Tauri release (`npm run desktop:build`)
9. ✅ Верификация созданных артефактов
10. ✅ Загрузка артефактов в GitHub Actions

### Время выполнения

- **Linux x86_64**: ~8-12 минут
- **Linux ARM64**: ~10-15 минут (зависит от доступности ARM runners)
- **Windows x86_64**: ~12-18 минут

Итого: **~10-20 минут** (сборки идут параллельно)

### Артефакты сборки

После успешного завершения workflow вы сможете скачать следующие артефакты:

#### Linux x86_64 (amd64)

**Artifact names в GitHub Actions:**
- `Newest-Launcher-linux-x86_64-deb`
- `Newest-Launcher-linux-x86_64-AppImage`

**Содержимое:**
- `Newest Launcher_0.1.0_amd64.deb` (~7 MB)
- `Newest Launcher_0.1.0_amd64.AppImage` (~80 MB)

**Установка:**
```bash
# DEB (Debian/Ubuntu)
sudo dpkg -i "Newest Launcher_0.1.0_amd64.deb"

# AppImage (универсальный)
chmod +x "Newest Launcher_0.1.0_amd64.AppImage"
./"Newest Launcher_0.1.0_amd64.AppImage"
```

#### Linux ARM64 (aarch64)

**Artifact names в GitHub Actions:**
- `Newest-Launcher-linux-aarch64-deb`
- `Newest-Launcher-linux-aarch64-AppImage`

**Содержимое:**
- `Newest Launcher_0.1.0_arm64.deb`
- `Newest Launcher_0.1.0_aarch64.AppImage`

**Установка:**
```bash
# DEB (Debian/Ubuntu на ARM64)
sudo dpkg -i "Newest Launcher_0.1.0_arm64.deb"

# AppImage (универсальный)
chmod +x "Newest Launcher_0.1.0_aarch64.AppImage"
./"Newest Launcher_0.1.0_aarch64.AppImage"
```

#### Windows x86_64

**Artifact names в GitHub Actions:**
- `Newest-Launcher-windows-x86_64-exe`
- `Newest-Launcher-windows-x86_64-msi` (опционально)

**Содержимое:**
- `Newest Launcher_0.1.0_x64-setup.exe` (NSIS installer)
- `Newest Launcher_0.1.0_x64_en-US.msi` (MSI installer, если создан)

**Установка:**
- Запустить `.exe` файл и следовать инструкциям установщика
- Или установить через `.msi` (двойной клик или `msiexec`)

### Как скачать артефакты

1. Откройте завершённый workflow run в GitHub Actions
2. Прокрутите вниз до секции **"Artifacts"**
3. Скачайте нужные zip-архивы
4. Распакуйте архивы для получения готовых установщиков

### Требования к GitHub Repository

#### Для Linux сборок (x86_64)
- ✅ Стандартный GitHub runner `ubuntu-latest` (всегда доступен)

#### Для Linux ARM64 сборок
- ⚠️ Требуется доступ к ARM64 runners
- Используется `ubuntu-24.04-arm`
- Для **бесплатных репозиториев**: ARM runners могут быть недоступны
- Для **платных/enterprise**: ARM runners обычно доступны

**Если ARM64 runner недоступен:**
- Job будет помечен как `skipped` или `failed` с понятным сообщением
- x86_64 и Windows сборки всё равно выполнятся (используется `fail-fast: false`)

#### Для Windows сборок
- ✅ Стандартный GitHub runner `windows-latest` (всегда доступен)

### Устранение проблем

#### ARM64 runner недоступен

**Симптом:**
```
Error: Unable to resolve action `ubuntu-24.04-arm`, 
unable to find version `ubuntu-24.04-arm`
```

**Решение:**
1. Убедитесь, что репозиторий имеет доступ к ARM runners
2. Или временно закомментируйте ARM64 job в `.github/workflows/build.yml`
3. Или используйте self-hosted ARM64 runner

#### Сборка не запускается

**Проверьте:**
- Workflow файл корректно размещён в `.github/workflows/build.yml`
- У вас есть права на запуск Actions в репозитории
- GitHub Actions включены в настройках репозитория

#### Артефакты не создаются

**Если `if-no-files-found: error` останавливает workflow:**
- Проверьте логи сборки Tauri
- Убедитесь, что `npm run desktop:build` завершается успешно
- Проверьте, что bundle target корректен в `tauri.conf.json`

### Локальная сборка (для разработчиков)

Если нужно собрать локально без GitHub Actions:

```bash
# Установка зависимостей
npm ci

# Проверки
npm run typecheck
cargo check --workspace

# Сборка
npm run desktop:build

# Артефакты будут в:
# Linux: target/release/bundle/deb/ и target/release/bundle/appimage/
# Windows: target/release/bundle/nsis/ и target/release/bundle/msi/
```

### Следующие шаги

После тестирования артефактов вы можете:

1. **Создать GitHub Release вручную** и прикрепить артефакты
2. **Настроить автоматический Release** при создании git tag
3. **Добавить code signing** для Windows и macOS
4. **Настроить auto-updater** в Tauri для обновлений

Эти шаги будут выполняться отдельно после проверки текущей сборки.

---

## Технические детали

### Matrix Strategy

Workflow использует GitHub Actions matrix для параллельной сборки:

```yaml
strategy:
  fail-fast: false
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

### Верификация артефактов

Workflow автоматически проверяет корректность созданных файлов:

**Linux:**
- `dpkg-deb --info` для проверки .deb структуры
- `file` для проверки AppImage типа
- Проверка executable bit на AppImage

**Windows:**
- Проверка существования .exe в bundle
- Listing всех созданных файлов

### Кэширование

- **npm cache**: автоматически через `actions/setup-node@v4` с `cache: 'npm'`
- **Rust cache**: рекомендуется добавить `Swatinem/rust-cache@v2` для ускорения

### Безопасность

- Используются официальные GitHub Actions (`@v4`)
- Используется официальный Rust toolchain от `dtolnay`
- Нет выполнения произвольного кода из внешних источников
- Все зависимости устанавливаются через package managers (npm, apt, cargo)

---

## Changelog

- **2026-09-19**: Создан multi-platform workflow с Linux x86_64, Linux ARM64, Windows x86_64
- Ручной запуск через `workflow_dispatch`
- Верификация артефактов перед upload
- Параллельная сборка с `fail-fast: false`
