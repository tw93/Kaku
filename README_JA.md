<div align="center">
  <img src="https://raw.githubusercontent.com/tw93/Kaku/main/assets/readme-logo.svg" width="120" />
  <h1>Kaku</h1>
  <p><em>実用的なデフォルト設定で、すぐに使える AI フレンドリーな Mac ターミナル。</em></p>
  <p><a href="README.md">English</a> · <a href="README_CN.md">中文</a> · <a href="README_TW.md">繁體</a> · 日本語 · <a href="README_KR.md">한국어</a> · <a href="README_DE.md">Deutsch</a> · <a href="README_FR.md">Français</a></p>
  <p>
    <a href="https://kaku.fun">公式サイト</a> ·
    <a href="https://kaku.fun/docs/">ドキュメント</a> ·
    <a href="https://kaku.fun/compare">比較</a> ·
    <a href="https://github.com/tw93/Kaku/releases/latest">ダウンロード</a>
  </p>
  <a href="https://kaku.fun"><img src="https://img.shields.io/badge/website-kaku.fun-1B365D?style=flat-square" alt="Website"></a>
  <a href="https://github.com/tw93/Kaku/stargazers"><img src="https://img.shields.io/github/stars/tw93/Kaku?style=flat-square" alt="Stars"></a>
  <a href="https://github.com/tw93/Kaku/releases"><img src="https://img.shields.io/github/v/tag/tw93/Kaku?label=version&style=flat-square" alt="Version"></a>
  <a href="LICENSE.md"><img src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square" alt="License"></a>
  <a href="https://github.com/tw93/Kaku/commits"><img src="https://img.shields.io/github/commit-activity/m/tw93/Kaku?style=flat-square" alt="Commits"></a>
  <a href="https://twitter.com/HiTw93"><img src="https://img.shields.io/badge/follow-Tw93-red?style=flat-square&logo=Twitter" alt="Twitter"></a>
</div>

<p align="center">
  <img src="assets/kaku.jpg" alt="Kaku スクリーンショット" width="1000" />
</p>

## 由来

Kaku（書く、かく）は日本語で「書く」という意味です。WezTerm をベースにした macOS ターミナルで、厳選されたフォント、テーマ、シェル統合、および馴染みのある Mac ショートカットがあらかじめ設定されています。さらにカスタマイズしたい場合は Lua 設定もそのまま利用できます。公式サイトは [kaku.fun](https://kaku.fun) です。

三部作のひとつ：[Kaku](https://github.com/tw93/Kaku)（書く）でコードを書き、[Waza](https://github.com/tw93/Waza)（技）で習慣を磨き、[Kami](https://github.com/tw93/Kami)（紙）で文書を届ける。家族に例えるなら、Kaku は父親、Waza は姉、Kami は妹のような存在です。

## クイックスタート

[Kaku DMG](https://github.com/tw93/Kaku/releases/latest) をダウンロードして開き、Kaku を「アプリケーション」フォルダにドラッグしてください。または Homebrew でインストールします：

```bash
brew install --cask kaku
```

Kaku を起動してシェル統合を完了します。不足しているオプションのツールは `kaku init` でインストールできます。インストールされているバージョンは `kaku --version` で確認できます。

## 特徴

- **すぐ使える**：JetBrains Mono フォント、明暗テーマの自動切り替え、選択時の自動コピー、馴染みのある Mac ショートカット
- **タブとペイン**：ワークスペースを自在に分割、Tab Navigator で目的のペインを素早く発見、Kaku を開き直したときにウィンドウ・ペイン・作業ディレクトリを自動復元
- **右クリックメニュー**：貼り付け、検索、AI チャット呼び出し、ペインの分割や終了など、ショートカットを覚えなくても直感的に操作可能
- **クリック可能なリンク**：`Cmd + クリック` で URL やファイルパスを直接オープン、改行された長い URL も途切れず認識
- **AI フレンドリー**：普段のコーディングツールに加えて、コマンド提案や対話ができるオプションのアシスタントも利用可能、`kaku ai` でお使いの AI サービスを設定可能
- **シェルツール群**：zsh 補完、シンタックスハイライト、ディレクトリジャンプを標準搭載、オプションでインストールする Lazygit や Yazi 用のショートカットも用意
- **Lua 設定**：WezTerm の Lua 設定システムを活用し、フォント、テーマ、キーバインド、動作を自由にカスタマイズ

## 使い方ガイド

| 操作 | ショートカット |
| :--- | :--- |
| 新規タブ | `Cmd + T` |
| 新規ウィンドウ | `Cmd + N` |
| タブ/ペインを閉じる | `Cmd + W` |
| タブ切り替え | `Cmd + Shift + [` / `]` または `Cmd + 1-9` |
| ペイン切り替え | `Cmd + Opt + 矢印キー` |
| 垂直分割 | `Cmd + D` |
| 水平分割 | `Cmd + Shift + D` |
| 設定パネルを開く | `Cmd + ,` |
| AI パネル | `Cmd + Shift + A` |
| AI チャット | `Cmd + L` |
| AI の提案を適用 | `Cmd + Shift + E` |
| Lazygit を開く | `Cmd + Shift + G` |
| Yazi ファイラー | `Cmd + Shift + Y` または `y` |
| 画面クリア | `Cmd + K` |
| コンテキストメニュー | ペイン内で右クリック |
| URL またはファイルを開く | `Cmd + クリック` |

詳細なキーバインド一覧：[docs/keybindings.md](docs/keybindings.md)

## Kaku AI

`kaku ai` でお使いの AI サービスを設定することで、内蔵アシスタントを利用できます。Kaku 自身は AI サービスを提供・中継しません。

- **コマンド提案**：コマンド実行が失敗した際、アシスタントが修正案を提示。`Cmd + Shift + E` でプロンプトに貼り付けて確認できます
- **自然言語からコマンド生成**：プロンプトで `# <説明>` と入力して Enter を押すと、生成されたコマンドがプロンプトに入力され、確認してから実行できます
- **AI チャット**：`Cmd + L` でターミナル出力について質問したり、プロジェクトファイルを活用した作業ができます。別シェルから `kaku chat` を実行して同じ対話履歴にアクセスすることも可能です
- **AI ツール設定**：Claude Code、Codex、Gemini CLI、Copilot CLI、Kimi Code などの設定を一元管理

認証、モデル、API モード、ツール設定の詳細は [AI アシスタントドキュメント](docs/features.md) を参照してください。

## よくある質問

**Windows や Linux 版はありますか？** 現在はありません。Kaku は現在 macOS 専用です。

**iTerm2、Warp、Ghostty、WezTerm との違いは何ですか？** 詳細は [kaku.fun/compare](https://kaku.fun/compare) をご覧ください。

**半透明ウィンドウは使えますか？** はい、`~/.config/kaku/kaku.lua` で `config.window_background_opacity` を設定してください。

**`kaku` コマンドが見つかりません。** `/Applications/Kaku.app/Contents/MacOS/kaku init --update-only && exec zsh -l` を実行後、`kaku doctor` で診断してください。

FAQ 全体：[docs/faq.md](docs/faq.md)

## ドキュメント

- [公式サイト](https://kaku.fun) - 製品サイト、インストール方法、英語・中国語ドキュメント
- [比較表](https://kaku.fun/compare) - Kaku と iTerm2、Warp、Ghostty、WezTerm、標準ターミナルの比較
- [キーバインド](docs/keybindings.md) - 全ショートカット一覧
- [機能一覧](docs/features.md) - AI アシスタント、Lazygit、Yazi、リモートファイル、シェル統合
- [設定方法](docs/configuration.md) - テーマ、フォント、キーバインド変更、Lua API
- [CLI リファレンス](docs/cli.md) - `kaku ai`、`kaku config`、`kaku doctor` などのコマンド
- [FAQ](docs/faq.md) - よくある質問とトラブルシューティング

## 開発の背景

仕事でも個人プロジェクトでも、私は CLI に深く依存しています。[Mole](https://github.com/tw93/mole) や [Pake](https://github.com/tw93/pake) といったツールにもその思想が表れています。

私は長年 Alacritty を愛用し、その速度とシンプルさを高く評価してきました。自分のワークフローが AI 支援コーディングへ移るにつれて、タブやペインの操作性にもっとこだわるようになりました。Kitty、Ghostty、Warp、iTerm2 も試しましたが、パフォーマンス、初期設定の完成度、カスタマイズの自由度が自身の理想と完全に合致する環境を求め続けました。

WezTerm は極めて堅牢で柔軟な拡張性を備えており、そのエンジンとエコシステムに心から感謝しています。Kaku は、まさにそうした理想の環境を実現するために作られました。速く、洗練されていて、すぐに仕事に使えます。

## コントリビューター

Kaku の構築に貢献してくれたすべての皆様に感謝します。ぜひフォローしてください！❤️

<a href="https://github.com/tw93/Kaku/graphs/contributors">
  <img src="./CONTRIBUTORS.svg?v=2" width="1000" />
</a>

## サポート

- 開発者を直接支援する方法として、Mac クリーナーアプリ [Mole for Mac](https://mole.fit) の購入をご検討ください
- Kaku が役に立ったら、Star を付けたり、[共有](https://twitter.com/intent/tweet?url=https://github.com/tw93/Kaku&text=Kaku%20-%20An%20AI-friendly%20Mac%20terminal.)したり、Issue や PR をお寄せください
- 私にはタンユエン（湯円）とコーラ（可楽）という2匹の猫がいます。Kaku を気に入っていただけたら、<a href="https://cats.tw93.fun?name=Kaku" target="_blank">缶詰 🥩</a> をプレゼントしていただけると嬉しいです

<details>
<summary>支援してくださった方々 🐱</summary>
<br/>
<a href="https://cats.tw93.fun?name=Kaku"><img src="https://cdn.jsdelivr.net/gh/tw93/sponsors@main/assets/sponsors.svg" width="1000" loading="lazy" /></a>
</details>

## ライセンス

MIT License。オープンソースをぜひお楽しみください。WezTerm および同梱フォントの帰属表示は [NOTICE.md](NOTICE.md) に記載されています。
