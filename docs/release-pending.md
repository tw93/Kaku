# Release pending

Work an earlier session left for the next release. The release reads this before drafting notes, does each item, and deletes its entry in the release commit, or writes in the entry why it waits for a later release.

- **Two-line prompts after a resize** (a351006d, K-46): add a release note item, for example that a two-line zsh prompt such as Powerlevel10k no longer stacks into several copies when the window width changes. The p10k part ships in `kaku.zsh`, so it needs this release's `config_version` bump. Then reply, where they wrote in, to the user who reported it with two screenshots (Ghostty on the left, Kaku on the right; they bought Mole and Pake) that it is fixed, and that `config.line_height = 1.0` in `~/.config/kaku/kaku.lua` makes the OpenCode logo match Ghostty.
