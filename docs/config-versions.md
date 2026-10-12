# Config Version History

`config_version` is a monotonically increasing integer. The single source of truth is
`assets/shell-integration/config_version.txt`; the release gate
(`scripts/check_release_config.sh`) requires it to increment on every release, even when
nothing in the schema changed. Do not hardcode the current number in agent guides or
instruction files; read the txt file instead.

## Rules for bumping

- A schema change must update the bundled defaults (`assets/macos/Kaku.app/Contents/Resources/kaku.lua`),
  user docs, release checks, and migration behavior together, in one change.
- Only keys that existed in the previous released version need migration code. A feature
  introduced in the current cycle ships its default directly: no migration, no version-gated
  branches, no release-note migration mention. Check with
  `git show V<previous release>:assets/macos/Kaku.app/Contents/Resources/kaku.lua | grep <key>`.
- A bump with no schema change is normal and expected; record why in this file.

## History

| Version | Release | Change |
|---|---|---|
| v21 | - | Adds `smart_tab_mode`. Introduces the optional `SmartPrompt` value for `window_close_confirmation` (the bundled default later flipped to `SmartPrompt` so a stateful pane is no longer dropped silently on Cmd+Q). Accepts the removed `language` option as a deprecated field for backward compat. |
| v22 | - | Adds a precmd guard so the dark-theme comment color override still applies when the user pre-loads fast-syntax-highlighting or zsh-syntax-highlighting in their own `.zshrc`. |
| v23 | - | Flips the bundled `smart_tab_mode` default to `suggestion_first` so Tab accepts a visible autosuggestion, falling back to completion. No schema change, so no migration; users who set `completion_first` keep it. |
| v24 | - | Migrates `$schema` keys in user yazi configs to the `#:schema` comment form that yazi 26.5.6+ requires. No schema change; the repair runs in three places that must stay in sync: setup scripts, the yazi wrapper, and `kaku.lua`. |
| v25 | - | No schema change. Bumps so an updated install regenerates the bundled zsh integration, picking up the fast-syntax-highlighting `path-to-dir` style that no longer underlines existing directories. |
| v26 | V0.12.4 | No schema or bundled-integration change; increments only because the release gate requires a bump every release. The release highlight reuses the Cmd+Click-opens-links improvement. |
| v27 | V0.13.0 | Adds `tab_title_show_foreground_process`, an opt-in setting for showing foreground process names in auto-generated tab titles. No migration is needed because this key did not exist in V0.12.4. |
| v28 | - | No schema change. Bumps so existing installs regenerate the bundled zsh integration with the self-contained SSH wrapper required by shell snapshot tools. |
| v29 | V0.15.0 | No schema change. Bumps so existing installs regenerate the bundled shell integrations with the aligned ssh wrappers: fish keeps user-defined ssh functions and gains the 1Password fix, env-prefix ssh aliases work, and mosh gets the same TERM fallback. |
| v30 | V0.16.0 | No schema change. Bumps so existing installs regenerate shell integrations that authenticate inline AI control messages with a local capability before terminal output can trigger assistant requests, and that scope the Starship prompt setup to Kaku instead of every zsh. |
| v31 | V0.17.0 | Adds the optional `file_link_editor` setting for choosing which editor opens local terminal file links, bare-domain Cmd+Click links (e.g. `github.com` without a scheme), and Option+Click cursor movement within the current input line. Also regenerates shell integrations with the #511 fixes: the inline AI capability file is read tolerantly (zsh/fish `read` treat a missing trailing newline as EOF failure), user-var guards report failure with `return 1` instead of masking it, and the fish widget only enters the waiting state after the request was actually emitted. The Rust side also writes the capability file with a trailing newline for new installs. |
| v32 | V0.18.0 | No schema change, and no bundled-integration change either; increments only because the release gate requires a bump every release. The release highlights reuse the new Move Tab to New Window command and the multi-display menu bar clamp fix. |
| v33 | V0.19.0 | No schema change. Bumps so existing installs pick up two bundled fixes: light-theme selected rows keep a visibly blue background instead of washing out to near-white, and the lazygit launcher resolves its working directory from the shell that actually receives the input, so it also works inside nested shells. |
| v34 | V0.20.0 | No schema change. Regenerates shell integrations to reset mouse reporting at the prompt after abnormal TUI or SSH exits, preserve the Kaku prompt and Smart Tab in tmux, and correct fish grep completion expansion. |
| v35 | V0.21.0 | No schema change. The config generated on first launch now sets `color_scheme = 'Kaku Dark'`; existing configs keep the bundled default that follows macOS appearance, so no migration is needed. Also increments because the release gate requires a bump every release. |
| v36 | V0.22.0 | Adds the opt-in `tab_title_use_pane_title` key and the opt-in `BuiltinInsert` value for `ime_preedit_rendering`; both defaults are unchanged. The bundled `show_new_tab_button_in_tab_bar` now defaults to true, and configs that set it keep their value, so no migration is needed. Bumps so existing installs regenerate `kaku.zsh` and stop writing before the first prompt (#561). |

When you bump the version, add a row here in the same change.
