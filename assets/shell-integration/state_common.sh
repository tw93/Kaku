# Shared shell state helpers for first-run and config updates.

read_bundled_config_version() {
	local script_dir="$1"
	local version_file="$script_dir/config_version.txt"

	if [[ ! -f "$version_file" ]]; then
		echo "Error: missing bundled config version file: $version_file" >&2
		return 1
	fi

	local version
	version="$(tr -d '[:space:]' < "$version_file" || true)"
	if [[ "$version" =~ ^[0-9]+$ ]]; then
		printf '%s\n' "$version"
		return 0
	fi

	echo "Error: invalid bundled config version in $version_file" >&2
	return 1
}

print_config_update_highlights() {
	local script_dir="$1"
	local from_version="$2"
	local target_version="$3"
	local highlights_file="$script_dir/config_update_highlights.tsv"
	local found=1
	local seen=$'\n'
	local wrap_width=72
	local current_group=""

	local GREEN=$'\033[0;32m'
	local DIM=$'\033[2m'
	local NC=$'\033[0m'

	if [[ ! -f "$highlights_file" ]]; then
		return 1
	fi

	while IFS=$'\t' read -r version highlight; do
		if [[ -z "${version:-}" || "$version" == \#* || -z "${highlight:-}" ]]; then
			continue
		fi

		if [[ "$version" =~ ^[0-9]+$ ]] && (( version > from_version && version <= target_version )); then
			if [[ "$seen" == *$'\n'"$highlight"$'\n'* ]]; then
				continue
			fi
			seen+="$highlight"$'\n'
			found=0

			if [[ "$version" != "$current_group" ]]; then
				current_group="$version"
				printf "  ${DIM}─── v%s ${NC}\n" "$version"
			fi

			printf '%s\n' "$highlight" | fold -s -w "$wrap_width" | \
				sed "1s|^|  ${GREEN}✦${NC} |; 2,\$s|^|    |"
		fi
	done < "$highlights_file"

	return "$found"
}

read_config_version() {
	if [[ ! -f "$STATE_FILE" ]]; then
		printf '%s\n' "0"
		return
	fi

	local version
	version="$(sed -nE 's/.*"config_version"[[:space:]]*:[[:space:]]*([0-9]+).*/\1/p' "$STATE_FILE" | head -n 1)"
	if [[ "$version" =~ ^[0-9]+$ ]]; then
		printf '%s\n' "$version"
	else
		printf '%s\n' "0"
	fi
}

persist_config_version() {
	local target_version="${1:-$CURRENT_CONFIG_VERSION}"
	mkdir -p "$CONFIG_DIR"

	local width height geometry_json
	width=""
	height=""
	geometry_json=""

	if [[ -f "$STATE_FILE" ]]; then
		width="$(sed -nE 's/.*"width"[[:space:]]*:[[:space:]]*([0-9]+).*/\1/p' "$STATE_FILE" | head -n 1)"
		height="$(sed -nE 's/.*"height"[[:space:]]*:[[:space:]]*([0-9]+).*/\1/p' "$STATE_FILE" | head -n 1)"
	fi

	if [[ -z "$width" || -z "$height" ]] && [[ -f "$LEGACY_GEOMETRY_FILE" ]]; then
		local geometry
		geometry="$(tr -d '[:space:]' < "$LEGACY_GEOMETRY_FILE" || true)"
		local a b c d
		IFS=',' read -r a b c d <<< "$geometry"
		if [[ "${c:-}" =~ ^[0-9]+$ && "${d:-}" =~ ^[0-9]+$ ]]; then
			width="$c"
			height="$d"
		elif [[ "${a:-}" =~ ^[0-9]+$ && "${b:-}" =~ ^[0-9]+$ ]]; then
			width="$a"
			height="$b"
		fi
	fi

	if [[ -n "$width" && -n "$height" ]]; then
		geometry_json="$(printf ',\n  "window_geometry": {\n    "width": %s,\n    "height": %s\n  }' "$width" "$height")"
	fi

	printf "{\n  \"config_version\": %s%s\n}\n" "$target_version" "$geometry_json" >"$STATE_FILE"

	# Keep a legacy version marker for users still loading older bundled kaku.lua.
	# This avoids repeated first-run onboarding after upgrades.
	printf '%s\n' "$target_version" >"$LEGACY_VERSION_FILE"
	rm -f "$LEGACY_GEOMETRY_FILE"
}

detect_login_shell() {
	if [[ -n "${SHELL:-}" && -x "${SHELL:-}" ]]; then
		printf '%s\n' "$SHELL"
		return
	fi

	local current_user resolved_shell passwd_entry
	current_user="${USER:-}"
	if [[ -z "$current_user" ]]; then
		current_user="$(id -un 2>/dev/null || true)"
	fi

	if [[ -n "$current_user" ]] && command -v dscl &>/dev/null; then
		resolved_shell="$(dscl . -read "/Users/$current_user" UserShell 2>/dev/null | awk '/UserShell:/ { print $2 }')"
		if [[ -n "$resolved_shell" && -x "$resolved_shell" ]]; then
			printf '%s\n' "$resolved_shell"
			return
		fi
	fi

	if [[ -n "$current_user" ]] && command -v getent &>/dev/null; then
		passwd_entry="$(getent passwd "$current_user" 2>/dev/null || true)"
		resolved_shell="${passwd_entry##*:}"
		if [[ -n "$resolved_shell" && -x "$resolved_shell" ]]; then
			printf '%s\n' "$resolved_shell"
			return
		fi
	fi

	if [[ -x "/bin/zsh" ]]; then
		printf '%s\n' "/bin/zsh"
	else
		printf '%s\n' "/bin/sh"
	fi
}
