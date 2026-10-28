# Translating Pulsar

Pulsar speaks English, Spanish and Simplified Chinese. It follows the Windows display language until you pick one under **Settings → General → Language**.

## How it works

The English text in the source is the key. Rust code wraps it in `tr!`, and `.slint` files in `@tr`:

```rust
tray.warn(tr!("Pulsar could not save settings"), &e.to_string());
let caption = tr!("{} of {}", used, total);
```

```slint
title: @tr("Start with Windows");
text: @tr("Pulsar {}", root.version);
```

Each crate keeps one gettext catalog per language:

| File | Holds |
|---|---|
| `crates/pulsar-core/locales/<code>/LC_MESSAGES/pulsar-monitor-core.po` | Taskbar labels, sensor and source errors, window titles |
| `crates/pulsar-monitor/locales/<code>/LC_MESSAGES/pulsar-monitor.po` | Menus, notifications, the hover popups, and the settings and details windows |
| `crates/*/locales/*.pot` | Templates with every message, for starting a new language |
| `installer/messages/<code>.isl` | The installer's own pages |

An empty `msgstr` shows the English. Placeholders are `{}` (the next value) or `{0}`, `{1}`… (by position, to reorder them); a translation must use the same ones as its English, which the tests check.

The catalogs are embedded in the executables, so nothing is read from disk at run time.

## Keeping catalogs in step with the code

`cargo test` fails when a catalog no longer matches the sources. To update them, run:

```powershell
$env:PULSAR_UPDATE_L10N = 1; cargo test --test catalogs; Remove-Item Env:PULSAR_UPDATE_L10N
```

This rewrites the templates, adds new messages to every catalog untranslated, and drops messages the code no longer uses. Translations and each catalog's header are kept.

## Adding a language

1. Add an entry to `LANGUAGES` in `crates/pulsar-core/src/l10n/mod.rs`: its code (a BCP 47 tag such as `fr` or `pt-BR`, also the folder name), its name in that language, and the Windows language tags it is chosen for. Set `font` if Segoe UI lacks the script, as Chinese does.
2. Copy each crate's `locales/*.pot` to `locales/<code>/LC_MESSAGES/` as a `.po` file with the same name, set `Language:` and `Plural-Forms:` in its header, and translate it. Any gettext editor, such as Poedit, works.
3. Copy `installer/messages/en.isl` to `<code>.isl` (saved as UTF-8 with a BOM), translate it, and add a line under `[Languages]` in `installer/pulsar.iss` with Inno Setup's own messages for the language. Add the translation of "Pulsar Settings" to `SettingsWindow` there too; the installer closes that window by its title.
4. Run `cargo test`, then check the settings window, the details window, the popups and the menu in the new language. Longer translations can be elided in narrow places such as the details sidebar.
