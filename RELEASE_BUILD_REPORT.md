# Release Build Report - Newest Launcher

**Date**: 19 сентября 2026  
**Task**: Сборка релизных бинарников без изменения функционала

---

## ✅ Статус выполнения

### Linux Builds: **УСПЕШНО**
- ✅ `.deb` пакет собран
- ✅ `.AppImage` собран

### Windows Build: **ПОДГОТОВЛЕН**
- ✅ GitHub Actions workflow создан
- ⏳ Требуется запуск в GitHub Actions

---

## 📦 Linux Артефакты (готовы к использованию)

### 1. DEB Package
- **Путь**: `target/release/bundle/deb/Newest Launcher_0.1.0_amd64.deb`
- **Размер**: 7.1 MB
- **Тип**: Debian binary package (format 2.0)
- **Архитектура**: amd64
- **Package Name**: newest-launcher
- **Version**: 0.1.0
- **Dependencies**: libwebkit2gtk-4.1-0, libgtk-3-0

### 2. AppImage
- **Путь**: `target/release/bundle/appimage/Newest Launcher_0.1.0_amd64.AppImage`
- **Размер**: 80 MB
- **Тип**: ELF 64-bit LSB pie executable, x86-64
- **Статус**: Исполняемый (executable bit установлен)
- **Формат**: Static-pie linked, stripped

---

## 🪟 Windows Build (GitHub Actions)

### Workflow файл
- **Путь**: `.github/workflows/build-windows.yml`
- **Trigger**: Manual (workflow_dispatch)
- **Platform**: windows-latest
- **Output**: `.exe` installer (NSIS)

### Инструкции по запуску
1. Запушить изменения в GitHub
2. Перейти в **Actions** → **Build Windows Release**
3. Нажать **Run workflow**
4. Скачать артефакт после завершения сборки

### Ожидаемые артефакты
- `newest-launcher-windows-exe` - содержит `.exe` установщик
- `newest-launcher-windows-msi` - опционально, содержит `.msi` установщик

---

## 🔧 Изменённые файлы (минимальные правки только для сборки)

### 1. `src-tauri/tauri.conf.json`
**Изменение**: Обновлена секция `bundle`
- Добавлены targets: изменено с `["deb"]` на `"all"` (для поддержки всех платформ)
- Добавлены icon paths (были пустыми)

**До**:
```json
"bundle": { "active": true, "targets": ["deb"], "icon": [] }
```

**После**:
```json
"bundle": {
  "active": true,
  "targets": "all",
  "icon": [
    "icons/32x32.png",
    "icons/64x64.png",
    "icons/128x128.png",
    "icons/128x128@2x.png",
    "icons/icon.icns",
    "icons/icon.ico"
  ]
}
```

### 2. `.github/workflows/build-windows.yml` (новый файл)
GitHub Actions workflow для автоматической сборки Windows .exe

### 3. `BUILD_INSTRUCTIONS.md` (новый файл)
Инструкции по сборке для разработчиков

### 4. `RELEASE_BUILD_REPORT.md` (этот файл)
Отчёт о выполненной сборке

---

## ✅ Проверка целостности проекта

### Код **НЕ БЫЛ** изменён
- ✅ Все Rust модули сохранены (11 файлов .rs)
- ✅ Все Frontend файлы сохранены (22 файла .ts/.tsx/.css)
- ✅ Все crates без изменений (launcher-core)
- ✅ Все assets сохранены (icons, изображения)
- ✅ Вся документация сохранена (docs/)
- ✅ Все скрипты сохранены (scripts/)

### Тесты и проверки пройдены
```bash
✅ npm ci                    # Зависимости установлены
✅ npm run typecheck        # TypeScript проверка пройдена
✅ cargo check --workspace  # Rust проверка пройдена
✅ cargo test --workspace   # Все тесты пройдены (18 passed, 3 ignored)
✅ npm run desktop:build    # Release сборка успешна
```

---

## 📊 Статистика сборки

### Время сборки (Linux)
- Frontend build (Vite): ~424ms
- Rust compilation: ~2m 16s
- Total build time: ~5 minutes (включая bundle)

### Размеры артефактов
- `.deb`: 7.1 MB (сжатый пакет)
- `.AppImage`: 80 MB (полностью self-contained)
- Release binary: ~14 MB (установленный размер)

---

## 🎯 Метаданные приложения (без изменений)

- **Product Name**: Newest Launcher
- **Version**: 0.1.0
- **Bundle Identifier**: io.newest.launcher
- **Window Title**: Newest Launcher
- **Default Size**: 1320x900 (минимум 900x600)

---

## 📝 Следующие шаги

1. **Для получения Windows .exe**:
   - Запушить код в GitHub репозиторий
   - Запустить workflow **Build Windows Release** вручную
   - Дождаться завершения (~10-15 минут)
   - Скачать артефакт из Actions

2. **Для тестирования Linux сборок**:
   - **DEB**: `sudo dpkg -i "target/release/bundle/deb/Newest Launcher_0.1.0_amd64.deb"`
   - **AppImage**: `./target/release/bundle/appimage/Newest\ Launcher_0.1.0_amd64.AppImage`

3. **Для повторной сборки**:
   ```bash
   npm run desktop:build
   ```

---

## ⚠️ Важные замечания

1. **Кодовая база сохранена полностью** - никакие модули не удалены, функционал не упрощён
2. **Архитектура не изменена** - все подсистемы (Minecraft Core, instances, OAuth, loaders) остались нетронутыми
3. **Только config изменения** - правки коснулись только Tauri конфигурации для корректной сборки
4. **Тесты проходят** - все 18 юнит-тестов успешны
5. **Build reproducible** - сборку можно повторить командой `npm run desktop:build`

---

## ✨ Итог

**Linux сборки готовы и находятся в**:
- `target/release/bundle/deb/Newest Launcher_0.1.0_amd64.deb`
- `target/release/bundle/appimage/Newest Launcher_0.1.0_amd64.AppImage`

**Windows сборка**:
- Workflow готов в `.github/workflows/build-windows.yml`
- Запустить вручную через GitHub Actions
- После выполнения скачать artifact

**Проект остался полностью функциональным без урезаний.**
