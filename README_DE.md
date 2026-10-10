<div align="center">
  <img src="https://raw.githubusercontent.com/tw93/Kaku/main/assets/readme-logo.svg" width="120" />
  <h1>Kaku</h1>
  <p><em>Ein KI-freundliches Mac-Terminal mit sinnvollen Voreinstellungen, sofort einsatzbereit.</em></p>
  <p><a href="README.md">English</a> · <a href="README_CN.md">中文</a> · <a href="README_TW.md">繁體</a> · <a href="README_JA.md">日本語</a> · <a href="README_KR.md">한국어</a> · Deutsch · <a href="README_FR.md">Français</a></p>
  <p>
    <a href="https://kaku.fun">Website</a> ·
    <a href="https://kaku.fun/docs/">Dokumentation</a> ·
    <a href="https://kaku.fun/compare">Vergleich</a> ·
    <a href="https://github.com/tw93/Kaku/releases/latest">Download</a>
  </p>
  <a href="https://kaku.fun"><img src="https://img.shields.io/badge/website-kaku.fun-1B365D?style=flat-square" alt="Website"></a>
  <a href="https://github.com/tw93/Kaku/stargazers"><img src="https://img.shields.io/github/stars/tw93/Kaku?style=flat-square" alt="Stars"></a>
  <a href="https://github.com/tw93/Kaku/releases"><img src="https://img.shields.io/github/v/tag/tw93/Kaku?label=version&style=flat-square" alt="Version"></a>
  <a href="LICENSE.md"><img src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square" alt="License"></a>
  <a href="https://github.com/tw93/Kaku/commits"><img src="https://img.shields.io/github/commit-activity/m/tw93/Kaku?style=flat-square" alt="Commits"></a>
  <a href="https://twitter.com/HiTw93"><img src="https://img.shields.io/badge/follow-Tw93-red?style=flat-square&logo=Twitter" alt="Twitter"></a>
</div>

<p align="center">
  <img src="assets/kaku.jpg" alt="Kaku Screenshot" width="1000" />
</p>

## Warum Kaku

Kaku (書く, かく) bedeutet auf Japanisch „schreiben“. Es ist ein auf WezTerm basierendes macOS-Terminal, das mit durchdachten Schriftarten, Themes, Shell-Integration und vertrauten Mac-Tastenkürzeln vorkonfiguriert ist, während die vollständige Lua-Konfiguration erhalten bleibt. Die Produkt-Website ist [kaku.fun](https://kaku.fun).

Als Coding-Fundament der Trilogie arbeitet Kaku mit dem auf Engineering-Gewohnheiten ausgerichteten [Waza](https://github.com/tw93/Waza) (技) und dem auf Dokumentenbereitstellung fokussierten [Kami](https://github.com/tw93/Kami) (紙) zusammen, um den Ablauf von der Entwicklung bis zur Auslieferung nahtlos zu gestalten.

## Schnellstart

Lade das [Kaku DMG](https://github.com/tw93/Kaku/releases/latest) herunter, öffne es und ziehe Kaku in den Programme-Ordner. Oder installiere es über Homebrew:

```bash
brew install --cask kaku
```

Öffne Kaku, um die Shell-Integration abzuschließen. Fehlende optionale Werkzeuge können mit `kaku init` installiert werden. Prüfe die installierte Version mit `kaku --version`.

## Funktionen

- **Sofort einsatzbereit**: JetBrains Mono, automatischer Hell-/Dunkelmodus, Kopieren bei Textauswahl und vertraute Mac-Tastenkürzel.
- **Tabs und Fensterteilung**: Arbeitsbereich flexibel teilen, Splits schnell über den Tab Navigator finden und Fenster, Splits und Arbeitsverzeichnisse beim erneuten Öffnen von Kaku automatisch wiederherstellen.
- **Kontextmenü per Rechtsklick**: Einfügen, Suchen, KI-Chat öffnen, Splits teilen oder schließen – ganz ohne Tastenkürzel auswendig lernen zu müssen.
- **Klickbare Links**: `Cmd + Klick` öffnet URLs und Dateipfade direkt; auch durch Zeilenumbrüche getrennte Links bleiben vollständig erhalten.
- **KI-freundlich**: Nutze deine Coding-Tools zusammen mit einem optionalen Assistenten für Befehlsvorschläge und Chat. Binde deinen eigenen KI-Dienst mit `kaku ai` an.
- **Shell-Werkzeuge**: Integrierte zsh-Autovervollständigung, Syntaxhervorhebung und Verzeichnisnavigation, mit Tastenkürzeln für optional installiertes Lazygit und Yazi.
- **Lua-Konfiguration**: Passe Schriften, Farbschemata, Tastenkürzel und Terminal-Verhalten flexibel über WezTerms Lua-System an.

## Bedienung

| Aktion | Tastenkürzel |
| :--- | :--- |
| Neuer Tab | `Cmd + T` |
| Neues Fenster | `Cmd + N` |
| Tab/Split schließen | `Cmd + W` |
| Tabs wechseln | `Cmd + Shift + [` / `]` oder `Cmd + 1-9` |
| Splits wechseln | `Cmd + Opt + Pfeiltasten` |
| Vertikal teilen | `Cmd + D` |
| Horizontal teilen | `Cmd + Shift + D` |
| Einstellungen öffnen | `Cmd + ,` |
| KI-Leiste öffnen | `Cmd + Shift + A` |
| KI-Chat | `Cmd + L` |
| KI-Vorschlag anwenden | `Cmd + Shift + E` |
| Lazygit öffnen | `Cmd + Shift + G` |
| Yazi Dateimanager | `Cmd + Shift + Y` oder `y` |
| Bildschirm leeren | `Cmd + K` |
| Kontextmenü | Rechtsklick im Split |
| Link/Pfad öffnen | `Cmd + Klick` |

Vollständige Tastenkürzel-Übersicht: [docs/keybindings.md](docs/keybindings.md)

## Kaku AI

Richte deinen eigenen KI-Dienst mit `kaku ai` ein, um den integrierten Assistenten zu nutzen. Kaku stellt keinen eigenen KI-Dienst bereit und leitet keine Daten über eigene Server.

- **Befehlsvorschläge**: Schlägt ein Befehl fehl, bietet der Assistent Korrekturen an. Mit `Cmd + Shift + E` direkt in die Eingabezeile einfügen und prüfen.
- **Natürliche Sprache zu Befehl**: Tippe `# <Beschreibung>` an der Eingabeaufforderung und drücke Enter. Der Assistent generiert den Befehl zur Überprüfung.
- **Chat**: Drücke `Cmd + L`, um Terminal-Ausgaben zu analysieren oder mit Projektdateien zu arbeiten. Nutze `kaku chat` aus einer anderen Shell für dieselbe Historie.
- **KI-Werkzeugkonfiguration**: Einstellungen für Claude Code, Codex, Gemini CLI, Copilot CLI, Kimi Code und weitere zentral verwalten.

Informationen zu Authentifizierung, Modellen, API-Modus und Tools findest du in der [KI-Dokumentation](docs/features.md).

## FAQ

**Gibt es eine Windows- oder Linux-Version?** Derzeit nicht. Kaku ist aktuell exklusiv für macOS.

**Wie unterscheidet sich Kaku von iTerm2, Warp, Ghostty oder WezTerm?** Siehe [kaku.fun/compare](https://kaku.fun/compare).

**Kann ich transparente Fenster verwenden?** Ja, setze `config.window_background_opacity` in `~/.config/kaku/kaku.lua`.

**Der Befehl `kaku` fehlt im Terminal.** Führe `/Applications/Kaku.app/Contents/MacOS/kaku init --update-only && exec zsh -l` aus, danach `kaku doctor`.

Vollständige FAQ: [docs/faq.md](docs/faq.md)

## Dokumentation

- [Website](https://kaku.fun) - Produktseite, Installation und englische/chinesische Dokumentation
- [Vergleich](https://kaku.fun/compare) - Kaku im Vergleich zu iTerm2, Warp, Ghostty, WezTerm und Terminal.app
- [Tastenkürzel](docs/keybindings.md) - Vollständige Tastenkürzel-Referenz
- [Funktionen](docs/features.md) - KI-Assistent, Lazygit, Yazi, Remote-Dateien, Shell-Suite
- [Konfiguration](docs/configuration.md) - Farbschemata, Schriften, eigene Tastenkürzel, Lua-API
- [CLI-Referenz](docs/cli.md) - `kaku ai`, `kaku config`, `kaku doctor` und mehr
- [FAQ](docs/faq.md) - Häufige Fragen und Fehlerbehebung

## Hintergrund

Sowohl beruflich als auch bei eigenen Projekten arbeite ich intensiv im Terminal. Werkzeuge, die ich gebaut habe, wie [Mole](https://github.com/tw93/mole) und [Pake](https://github.com/tw93/pake), spiegeln genau diese Haltung wider.

Ich habe Alacritty jahrelang genutzt und dessen Geschwindigkeit und Schlichtheit schätzen gelernt. Als sich mein Arbeitsablauf in Richtung KI-unterstützter Entwicklung verlagerte, stiegen meine Ansprüche an ergonomische Tabs und Splits. Ich habe Kitty, Ghostty, Warp und iTerm2 ausgiebig getestet – alle haben ihre Stärken. Dennoch suchte ich eine Umgebung, die Leistung, vorkonfigurierte Qualität und volle Kontrolle perfekt vereint.

WezTerm ist enorm stabil und flexibel erweiterbar; ich schätze dessen Core und Ökosystem sehr. Genau daraus ist Kaku entstanden: schnell, durchdacht und sofort einsatzbereit.

## Mitwirkende

Vielen Dank an alle Mitwirkenden, die Kaku mitgestaltet haben. Folgt ihnen gern! ❤️

<a href="https://github.com/tw93/Kaku/graphs/contributors">
  <img src="./CONTRIBUTORS.svg?v=2" width="1000" />
</a>

## Unterstützung

- Die direkteste Unterstützung ist der Kauf von [Mole for Mac](https://mole.fit), meiner Bereinigungs-App für macOS.
- Wenn dir Kaku gefällt, vergib einen Stern, [teile es](https://twitter.com/intent/tweet?url=https://github.com/tw93/Kaku&text=Kaku%20-%20An%20AI-friendly%20Mac%20terminal.) oder eröffne ein Issue oder einen PR.
- Ich habe zwei Katzen, TangYuan und Coke. Wenn dir Kaku Freude bereitet, kannst du ihnen etwas <a href="https://cats.tw93.fun?name=Kaku" target="_blank">Dosenfutter 🥩</a> spendieren.

<details>
<summary>Unterstützer 🐱</summary>
<br/>
<a href="https://cats.tw93.fun?name=Kaku"><img src="https://cdn.jsdelivr.net/gh/tw93/sponsors@main/assets/sponsors.svg" width="1000" loading="lazy" /></a>
</details>

## Lizenz

MIT License. Please feel free to use and contribute to the development. Namensnennungen für WezTerm und die mitgelieferten Schriftarten finden sich in [NOTICE.md](NOTICE.md).
