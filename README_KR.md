<div align="center">
  <img src="https://raw.githubusercontent.com/tw93/Kaku/main/assets/readme-logo.svg" width="120" />
  <h1>Kaku</h1>
  <p><em>합리적인 기본 설정으로 바로 쓸 수 있는 AI 친화적인 Mac 터미널.</em></p>
  <p><a href="README.md">English</a> · <a href="README_CN.md">中文</a> · <a href="README_TW.md">繁體</a> · <a href="README_JA.md">日本語</a> · 한국어 · <a href="README_DE.md">Deutsch</a> · <a href="README_FR.md">Français</a></p>
  <p>
    <a href="https://kaku.fun">공식 웹사이트</a> ·
    <a href="https://kaku.fun/docs/">문서</a> ·
    <a href="https://kaku.fun/compare">비교</a> ·
    <a href="https://github.com/tw93/Kaku/releases/latest">다운로드</a>
  </p>
  <a href="https://kaku.fun"><img src="https://img.shields.io/badge/website-kaku.fun-1B365D?style=flat-square" alt="Website"></a>
  <a href="https://github.com/tw93/Kaku/stargazers"><img src="https://img.shields.io/github/stars/tw93/Kaku?style=flat-square" alt="Stars"></a>
  <a href="https://github.com/tw93/Kaku/releases"><img src="https://img.shields.io/github/v/tag/tw93/Kaku?label=version&style=flat-square" alt="Version"></a>
  <a href="LICENSE.md"><img src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square" alt="License"></a>
  <a href="https://github.com/tw93/Kaku/commits"><img src="https://img.shields.io/github/commit-activity/m/tw93/Kaku?style=flat-square" alt="Commits"></a>
  <a href="https://twitter.com/HiTw93"><img src="https://img.shields.io/badge/follow-Tw93-red?style=flat-square&logo=Twitter" alt="Twitter"></a>
</div>

<p align="center">
  <img src="assets/kaku.jpg" alt="Kaku 스크린샷" width="1000" />
</p>

## 소개

Kaku(書く, 카쿠)는 일본어로 '쓰다'를 뜻합니다. WezTerm 기반의 macOS 터미널로, 폰트, 테마, 셸 통합, 익숙한 Mac 단축키가 기본 설정되어 있으며 Lua 커스터마이징 기능도 온전히 제공합니다.

3부작 중 코딩을 담당하는 기본 터미널로서, 개발자 습관을 다듬는 [Waza](https://github.com/tw93/Waza) (技), 문서를 전달하는 [Kami](https://github.com/tw93/Kami) (紙)와 함께 코드 작성부터 습관 형성, 문서 납품까지 막힘없이 이어지도록 지원합니다.

## 빠른 시작

[Kaku DMG](https://github.com/tw93/Kaku/releases/latest)를 다운로드하여 열고 응용 프로그램 폴더로 드래그합니다. 또는 Homebrew로 설치할 수 있습니다:

```bash
brew install --cask kaku
```

Kaku를 실행하여 셸 통합을 완료합니다. 필요한 부가 도구는 `kaku init`으로 설치하고, 설치된 버전은 `kaku --version`으로 확인합니다.

## 주요 기능

- **즉시 사용 가능**: JetBrains Mono 폰트, 라이트/다크 테마 자동 전환, 선택 즉시 복사, 익숙한 Mac 단축키
- **탭 및 화면 분할**: 자유로운 작업 공간 분할, Tab Navigator를 통한 빠른 탭 전환, Kaku를 다시 열 때 창, 분할 화면, 작업 경로 자동 복원
- **우클릭 메뉴**: 붙여넣기, 검색, AI 대화 호출, 분할 화면 닫기 및 탭 관리를 단축키 암기 없이 손쉽게 실행
- **클릭 가능한 링크**: `Cmd + 클릭`으로 URL 및 파일 경로 열기 지원, 터미널 줄바꿈이 일어난 긴 링크도 완전하게 인식
- **AI 친화적**: 평소 쓰는 코딩 도구와 함께 명령어 제안 및 대화를 위한 선택형 어시스턴트 사용 가능
- **셸 도구 모음**: zsh 자동 완성, 구문 강조, 디렉터리 빠른 이동 내장, 선택 설치한 Lazygit 및 Yazi용 단축키 지원
- **Lua 설정**: WezTerm의 Lua 설정 시스템을 활용하여 폰트, 테마, 단축키, 동작 방식을 자유롭게 구성

## 사용 가이드

| 동작 | 단축키 |
| :--- | :--- |
| 새 탭 | `Cmd + T` |
| 새 창 | `Cmd + N` |
| 탭/분할창 닫기 | `Cmd + W` |
| 탭 전환 | `Cmd + Shift + [` / `]` 또는 `Cmd + 1-9` |
| 분할창 이동 | `Cmd + Opt + 방향키` |
| 수직 분할 | `Cmd + D` |
| 수평 분할 | `Cmd + Shift + D` |
| 환경설정 패널 열기 | `Cmd + ,` |
| AI 패널 | `Cmd + Shift + A` |
| AI 대화 | `Cmd + L` |
| AI 제안 적용 | `Cmd + Shift + E` |
| Lazygit 열기 | `Cmd + Shift + G` |
| Yazi 파일 탐색기 | `Cmd + Shift + Y` 또는 `y` |
| 화면 지우기 | `Cmd + K` |

## Kaku AI

`kaku ai`로 사용할 AI 서비스를 설정하면 내장 어시스턴트를 활용할 수 있습니다. Kaku 자체는 AI 서비스를 직접 제공하거나 중계하지 않습니다.

- **명령어 제안**: 명령어 실행 실패 시 어시스턴트가 해결책 제안, `Cmd + Shift + E`로 프롬프트에 붙여넣어 확인 후 실행
- **자연어 명령어 변환**: 프롬프트에 `# <설명>` 입력 후 Enter를 누르면 어시스턴트가 생성된 명령어를 프롬프트에 입력하며, 확인 후 실행할 수 있습니다
- **AI 대화**: `Cmd + L`로 터미널 출력에 대해 질문하거나 프로젝트 파일 및 도구와 함께 작업, 다른 셸에서 `kaku chat`을 실행해 동일한 대화 기록 접근 가능
- **AI 도구 설정**: Claude Code, Codex, Gemini CLI, Copilot CLI, Kimi Code 등의 설정을 한곳에서 관리

인증, 모델, API 모드 및 도구 설정에 관한 자세한 내용은 [AI 어시스턴트 문서](docs/features.md)를 참조하세요.

## 자주 묻는 질문 (FAQ)

- **Windows 또는 Linux 버전이 있나요?** 현재는 없습니다. Kaku는 macOS 전용입니다
- **iTerm2, Warp, Ghostty, WezTerm과의 차이점은 무엇인가요?** [kaku.fun/compare](https://kaku.fun/compare)를 확인하세요
- **반투명 창을 사용할 수 있나요?** 가능합니다. `~/.config/kaku/kaku.lua`에서 `config.window_background_opacity`를 설정하세요
- **`kaku` 명령어를 찾을 수 없습니다.** `/Applications/Kaku.app/Contents/MacOS/kaku init --update-only && exec zsh -l`을 실행한 뒤 `kaku doctor`로 점검하세요

## 문서

- [단축키](docs/keybindings.md) - 전체 단축키 레퍼런스
- [기능 소개](docs/features.md) - AI 어시스턴트, Lazygit, Yazi, 원격 파일, 셸 도구
- [환경설정](docs/configuration.md) - 테마, 폰트, 커스텀 단축키, Lua API
- [CLI 레퍼런스](docs/cli.md) - `kaku ai`, `kaku config`, `kaku doctor` 등
- [FAQ](docs/faq.md) - 자주 묻는 질문 및 문제 해결

## 개발 배경

업무와 개인 프로젝트 모두에서 저는 명령줄에 크게 의존합니다. 제가 만든 [Mole](https://github.com/tw93/mole)과 [Pake](https://github.com/tw93/pake) 같은 도구에도 그 점이 드러납니다.

오랫동안 Alacritty를 쓰면서 속도와 단순함을 중요하게 여기게 되었습니다. 작업 방식이 AI 지원 코딩으로 옮겨 가면서 탭과 분할 화면을 더 편하게 다루고 싶어졌습니다. Kitty, Ghostty, Warp, iTerm2도 써 보았고 각자 강점이 있었지만, 성능, 기본 설정, 제어 사이에서 제 기준에 맞는 균형을 갖춘 환경을 여전히 원했습니다.

WezTerm은 견고하고 손보기 좋으며, 그 엔진과 생태계에 감사하고 있습니다. 그래서 빠르고, 깔끔하며, 바로 일할 수 있는 환경으로 Kaku를 만들었습니다.

## 기여자

Kaku 개발에 기여해 주신 모든 분들께 감사드립니다. 이분들을 팔로우해 주세요! ❤️

<a href="https://github.com/tw93/Kaku/graphs/contributors">
  <img src="./CONTRIBUTORS.svg?v=2" width="1000" />
</a>

## 후원

- 제가 만든 유료 Mac 정리 앱 [Mole for Mac](https://mole.fit)을 이용해 주시는 것이 가장 직접적인 후원입니다
- Kaku가 유용했다면 Star를 누르거나, [공유](https://twitter.com/intent/tweet?url=https://github.com/tw93/Kaku&text=Kaku%20-%20An%20AI-friendly%20Mac%20terminal.)하거나, 이슈 및 PR을 남겨주세요
- 탕위안(TangYuan)과 콜라(Coke)라는 두 마리의 고양이를 키우고 있는데, Kaku가 즐거움을 주었다면 <a href="https://cats.tw93.fun?name=Kaku" target="_blank">캔 간식 🥩</a>을 후원해 주세요

<details>
<summary>후원해 주신 분들 🐱</summary>
<br/>
<a href="https://cats.tw93.fun?name=Kaku"><img src="https://cdn.jsdelivr.net/gh/tw93/sponsors@main/assets/sponsors.svg" width="1000" loading="lazy" /></a>
</details>

## 라이선스

MIT License. Please feel free to use and contribute to the development. WezTerm 및 포함된 폰트에 대한 저작권 고지는 [NOTICE.md](NOTICE.md)에 명시되어 있습니다.
