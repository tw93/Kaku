<div align="center">
  <img src="https://raw.githubusercontent.com/tw93/Kaku/main/assets/readme-logo.svg" width="120" />
  <h1>Kaku</h1>
  <p><em>Un terminal Mac adapté à l'IA, avec des réglages par défaut judicieux, prêt à l'emploi.</em></p>
  <p><a href="README.md">English</a> · <a href="README_CN.md">中文</a> · <a href="README_TW.md">繁體</a> · <a href="README_JA.md">日本語</a> · <a href="README_KR.md">한국어</a> · <a href="README_DE.md">Deutsch</a> · Français</p>
  <p>
    <a href="https://kaku.fun">Site web</a> ·
    <a href="https://kaku.fun/docs/">Documentation</a> ·
    <a href="https://kaku.fun/compare">Comparatif</a> ·
    <a href="https://github.com/tw93/Kaku/releases/latest">Télécharger</a>
  </p>
  <a href="https://kaku.fun"><img src="https://img.shields.io/badge/website-kaku.fun-1B365D?style=flat-square" alt="Website"></a>
  <a href="https://github.com/tw93/Kaku/stargazers"><img src="https://img.shields.io/github/stars/tw93/Kaku?style=flat-square" alt="Stars"></a>
  <a href="https://github.com/tw93/Kaku/releases"><img src="https://img.shields.io/github/v/tag/tw93/Kaku?label=version&style=flat-square" alt="Version"></a>
  <a href="LICENSE.md"><img src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square" alt="License"></a>
  <a href="https://github.com/tw93/Kaku/commits"><img src="https://img.shields.io/github/commit-activity/m/tw93/Kaku?style=flat-square" alt="Commits"></a>
  <a href="https://twitter.com/HiTw93"><img src="https://img.shields.io/badge/follow-Tw93-red?style=flat-square&logo=Twitter" alt="Twitter"></a>
</div>

<p align="center">
  <img src="assets/kaku.jpg" alt="Capture d'écran de Kaku" width="1000" />
</p>

## Pourquoi Kaku

Kaku (書く, かく) signifie « écrire » en japonais. C'est un terminal macOS basé sur WezTerm, livré avec des polices de qualité, des thèmes soignés, l'intégration du shell et des raccourcis Mac familiers déjà configurés, tout en conservant une personnalisation Lua complète. Le site officiel est [kaku.fun](https://kaku.fun).

En tant que socle de programmation de la trilogie, Kaku s'associe à [Waza](https://github.com/tw93/Waza) (技) dédié aux bonnes pratiques et à [Kami](https://github.com/tw93/Kami) (紙) dédié à la livraison de documents pour assurer une expérience fluide du code jusqu'au rendu final.

## Démarrage rapide

Téléchargez le fichier [DMG de Kaku](https://github.com/tw93/Kaku/releases/latest), ouvrez-le et glissez Kaku dans le dossier Applications. Ou installez-le avec Homebrew :

```bash
brew install --cask kaku
```

Lancez Kaku pour finaliser la configuration du shell. Les outils optionnels manquants peuvent être installés via `kaku init`. Vérifiez la version installée avec `kaku --version`.

## Fonctionnalités

- **Prêt à l'emploi** : police JetBrains Mono, thèmes clair et sombre automatiques, copie à la sélection et raccourcis Mac familiers.
- **Onglets et volets** : séparez facilement votre espace de travail, repérez un volet grâce au Tab Navigator et restaurez vos fenêtres, volets et répertoires à la réouverture de Kaku.
- **Menu contextuel au clic droit** : coller, rechercher, lancer le chat IA, diviser ou fermer un volet sans avoir à mémoriser de combinaisons.
- **Liens interactifs** : `Cmd + Clic` ouvre directement les URL et chemins de fichiers ; les liens longs coupés sur plusieurs lignes restent entièrement cliquables.
- **Pensé pour l'IA** : utilisez vos outils de développement avec un assistant optionnel pour suggérer des commandes et dialoguer. Configurez votre propre service d'IA avec `kaku ai`.
- **Suite d'outils shell** : autocomplétion zsh, coloration syntaxique et navigation rapide intégrées, avec des raccourcis pour Lazygit et Yazi, à installer en option.
- **Configuration en Lua** : personnalisez polices, thèmes, raccourcis et comportements grâce au moteur Lua de WezTerm.

## Guide d'utilisation

| Action | Raccourci |
| :--- | :--- |
| Nouvel onglet | `Cmd + T` |
| Nouvelle fenêtre | `Cmd + N` |
| Fermer onglet/volet | `Cmd + W` |
| Naviguer entre onglets | `Cmd + Shift + [` / `]` ou `Cmd + 1-9` |
| Naviguer entre volets | `Cmd + Opt + Flèches` |
| Séparation verticale | `Cmd + D` |
| Séparation horizontale | `Cmd + Shift + D` |
| Ouvrir les paramètres | `Cmd + ,` |
| Panneau IA | `Cmd + Shift + A` |
| Chat IA | `Cmd + L` |
| Appliquer la suggestion IA | `Cmd + Shift + E` |
| Ouvrir Lazygit | `Cmd + Shift + G` |
| Gestionnaire Yazi | `Cmd + Shift + Y` ou `y` |
| Effacer l'écran | `Cmd + K` |
| Menu contextuel | Clic droit dans un volet |
| Ouvrir lien ou fichier | `Cmd + Clic` |

Référence complète des raccourcis : [docs/keybindings.md](docs/keybindings.md)

## Kaku AI

Configurez votre propre fournisseur d'IA avec `kaku ai` pour activer l'assistant intégré. Kaku ne fournit ni ne relaie aucun service d'IA.

- **Suggestions de commandes** : lorsqu'une commande échoue, l'assistant propose un correctif. Appuyez sur `Cmd + Shift + E` pour l'insérer sur la ligne de commande et le vérifier.
- **Langage naturel vers commande** : saisissez `# <description>` sur la ligne de commande et appuyez sur Entrée pour générer la commande correspondante, que vous pouvez vérifier avant de l'exécuter.
- **Chat** : appuyez sur `Cmd + L` pour échanger sur les sorties du terminal ou interagir avec vos fichiers de projet. Utilisez `kaku chat` depuis un autre terminal pour retrouver la même conversation.
- **Configuration des outils d'IA** : gérez les paramètres pour Claude Code, Codex, Gemini CLI, Copilot CLI, Kimi Code et d'autres outils.

Pour l'authentification, les modèles, le mode API et les réglages d'outils, consultez la [documentation de l'assistant IA](docs/features.md).

## FAQ

**Existe-t-il une version Windows ou Linux ?** Pas pour l'instant. Kaku est actuellement exclusif à macOS.

**En quoi Kaku diffère-t-il d'iTerm2, Warp, Ghostty ou WezTerm ?** Voir [kaku.fun/compare](https://kaku.fun/compare).

**Puis-je rendre la fenêtre transparente ?** Oui, configurez `config.window_background_opacity` dans `~/.config/kaku/kaku.lua`.

**La commande `kaku` est introuvable.** Exécutez `/Applications/Kaku.app/Contents/MacOS/kaku init --update-only && exec zsh -l`, puis vérifiez avec `kaku doctor`.

FAQ complète : [docs/faq.md](docs/faq.md)

## Documentation

- [Site officiel](https://kaku.fun) - site produit, installation et documentation en anglais / chinois
- [Comparatif](https://kaku.fun/compare) - Kaku face à iTerm2, Warp, Ghostty, WezTerm et Terminal.app
- [Raccourcis](docs/keybindings.md) - guide complet des touches
- [Fonctionnalités](docs/features.md) - assistant IA, lazygit, yazi, fichiers distants, shell suite
- [Configuration](docs/configuration.md) - thèmes, typographie, raccourcis personnalisés, API Lua
- [Référence CLI](docs/cli.md) - commandes `kaku ai`, `kaku config`, `kaku doctor`, etc.
- [FAQ](docs/faq.md) - questions fréquentes et résolution de problèmes

## Genèse du projet

Tant au travail que sur mes projets personnels, je dépends quotidiennement de la ligne de commande. Les outils que j'ai conçus, comme [Mole](https://github.com/tw93/mole) et [Pake](https://github.com/tw93/pake), en sont l'illustration directe.

J'ai utilisé Alacritty pendant des années en appréciant sa rapidité et sa pureté. Quand mon flux de travail s'est tourné vers le développement assisté par l'IA, j'ai eu besoin d'une meilleure ergonomie pour la gestion des onglets et volets. J'ai aussi testé Kitty, Ghostty, Warp et iTerm2 : chacun possède de grandes qualités, mais je cherchais l'équilibre idéal entre performances pures, configuration soignée par défaut et liberté totale.

WezTerm s'est révélé solide et particulièrement malléable, et je suis très reconnaissant pour son moteur et son écosystème. J'ai créé Kaku pour offrir exactement cela : un terminal rapide, soigné et prêt à l'emploi.

## Contributeurs

Un grand merci à toutes les personnes qui contribuent à Kaku. N'hésitez pas à les suivre ! ❤️

<a href="https://github.com/tw93/Kaku/graphs/contributors">
  <img src="./CONTRIBUTORS.svg?v=2" width="1000" />
</a>

## Soutien

- La manière la plus directe de me soutenir est d'acheter [Mole for Mac](https://mole.fit), mon application de nettoyage pour Mac.
- Si Kaku vous est utile, donnez-lui une étoile, [partagez-le](https://twitter.com/intent/tweet?url=https://github.com/tw93/Kaku&text=Kaku%20-%20An%20AI-friendly%20Mac%20terminal.) ou participez aux issues et PR.
- J'ai deux chats, TangYuan et Coke. Si Kaku embellit votre quotidien, vous pouvez leur offrir de la <a href="https://cats.tw93.fun?name=Kaku" target="_blank">pâtée 🥩</a>.

<details>
<summary>Ceux qui ont déjà participé 🐱</summary>
<br/>
<a href="https://cats.tw93.fun?name=Kaku"><img src="https://cdn.jsdelivr.net/gh/tw93/sponsors@main/assets/sponsors.svg" width="1000" loading="lazy" /></a>
</details>

## Licence

MIT License. Please feel free to use and contribute to the development. Les crédits pour WezTerm et les polices incluses sont détaillés dans [NOTICE.md](NOTICE.md).
