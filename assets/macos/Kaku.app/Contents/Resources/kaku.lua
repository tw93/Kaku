-- Kaku Configuration

local wezterm = require 'wezterm'

local config = {}

-- `config_builder` validates every assignment and is expensive on large configs.
-- Keep startup fast by default; enable strict validation only when debugging config.
if os.getenv('KAKU_STRICT_CONFIG') == '1' and wezterm.config_builder then
  config = wezterm.config_builder()
end

local function basename(path)
  return path:match('([^/]+)$')
end

local yazi_mode_hints = {}

local function pane_hint_key(pane)
  if not pane then
    return nil
  end

  local ok, pane_id = pcall(function()
    return pane:pane_id()
  end)
  if not ok or not pane_id then
    return nil
  end

  return tostring(pane_id)
end

local function set_yazi_mode_hint(pane, active)
  local key = pane_hint_key(pane)
  if not key then
    return
  end

  if active then
    yazi_mode_hints[key] = true
  else
    yazi_mode_hints[key] = nil
  end
end

-- URL decode helper for Chinese characters in paths
-- Converts %E9%9F%B3%E4%B9%90 -> 音乐
local function url_decode(str)
  if not str then
    return str
  end
  -- First, handle UTF-8 encoded sequences (%XX%YY%ZZ)
  local result = str:gsub('%%([0-9A-Fa-f][0-9A-Fa-f])', function(hex)
    return string.char(tonumber(hex, 16))
  end)
  return result
end

local function default_kaku_user_config_path()
  local xdg = os.getenv('XDG_CONFIG_HOME')
  if xdg and xdg ~= '' then
    return xdg .. '/kaku/kaku.lua'
  end
  local home = os.getenv('HOME')
  if home then
    return home .. '/.config/kaku/kaku.lua'
  end
  return nil
end

local function is_bundled_kaku_config_path(path)
  if type(path) ~= 'string' or path == '' then
    return false
  end

  local normalized = path:gsub('\\', '/')
  return normalized:match('/Kaku%.app/Contents/Resources/kaku%.lua$') ~= nil
    or normalized:match('/assets/macos/Kaku%.app/Contents/Resources/kaku%.lua$') ~= nil
end

-- Detect if user has custom config overrides in their config file.
-- Prefer the actively loaded config file, but ignore the bundled defaults file.
local function kaku_user_config_path()
  local runtime_config = nil
  if type(wezterm.config_file) == 'string' and wezterm.config_file ~= '' then
    runtime_config = wezterm.config_file
  end
  if (not runtime_config or runtime_config == '') then
    local env_config = os.getenv('KAKU_CONFIG_FILE')
    if env_config and env_config ~= '' then
      runtime_config = env_config
    end
  end

  if runtime_config and runtime_config ~= '' and not is_bundled_kaku_config_path(runtime_config) then
    return runtime_config
  end

  return default_kaku_user_config_path()
end

local user_has_custom_padding = false
local user_has_custom_font = false
local user_has_custom_font_rules = false
local remember_last_cwd = true

local function check_user_custom_config()
  local user_config_path = kaku_user_config_path()
  if not user_config_path then
    return
  end
  local file = io.open(user_config_path, 'r')
  if not file then
    return
  end
  -- Check if user explicitly sets these configs (skip comment lines).
  for line in file:lines() do
    local trimmed = line:match('^%s*(.-)%s*$')
    if trimmed and not trimmed:match('^%-%-') then
      if trimmed:match('^config%.window_padding%s*=') then
        user_has_custom_padding = true
      end
      if trimmed:match('^config%.font%s*=') then
        user_has_custom_font = true
      end
      if trimmed:match('^config%.font_rules%s*=') then
        user_has_custom_font_rules = true
      end
      if trimmed:match('^config%.remember_last_cwd%s*=%s*false') then
        remember_last_cwd = false
      end
    end
  end
  file:close()
end
check_user_custom_config()

local function resolve_appearance_color_scheme()
  local gui = wezterm.gui
  if not gui or type(gui.get_appearance) ~= 'function' then
    return 'Kaku Dark'
  end

  local ok, appearance = pcall(gui.get_appearance)
  if not ok or type(appearance) ~= 'string' then
    return 'Kaku Dark'
  end

  return appearance:find('Dark', 1, true) and 'Kaku Dark' or 'Kaku Light'
end

local function resolve_kaku_color_scheme(scheme)
  if scheme == 'Auto' then
    return resolve_appearance_color_scheme()
  end
  if not scheme or scheme == '' then
    return 'Kaku Dark'
  end
  return scheme
end

-- Two-tier display detection.
-- low resolution screens use smaller spacing and 15px font.
-- high resolution screens use default spacing and 17px font.
local function is_low_resolution_screen()
  local success, screens = pcall(function()
    return wezterm.gui.screens()
  end)
  if success and screens and screens.main then
    local main = screens.main
    local width = tonumber(main.width or 0) or 0
    local height = tonumber(main.height or 0) or 0
    local short_edge = math.min(width, height)
    -- Inline builtin screen detection.
    local name = string.lower(tostring(main.name or ''))
    local is_builtin = name == 'color lcd'
      or string.find(name, 'built-in', 1, true)
      or string.find(name, 'built in', 1, true)
      or string.find(name, '内建', 1, true)
    if short_edge > 0 then
      if is_builtin then
        return short_edge <= 1700
      end
      return short_edge < 1800
    end
  end
  return false
end

-- Compute once; all spacing helpers below share this result.
local low_resolution_screen = is_low_resolution_screen()

local function get_default_padding()
  if low_resolution_screen then
    return { left = '26px', right = '26px', top = '26px', bottom = '0px' }
  end
  return { left = '40px', right = '40px', top = '40px', bottom = '0px' }
end

-- get_fullscreen_padding and get_yazi_fullscreen_padding have been removed.
-- Fullscreen padding adjustments are now handled synchronously in Rust (resize.rs)
-- to avoid async layout jitter.

-- Per-window resize debounce state.
-- Weak keys ensure closed windows don't leak state.
local resize_state_by_window = setmetatable({}, { __mode = 'k' })

local function monotonic_now()
  -- Keep this numeric for debounce arithmetic.
  -- Some runtime environments expose os.clock() as a non-number value.
  local ok, value = pcall(os.clock)
  if ok and type(value) == 'number' then
    return value
  end
  return os.time()
end

local function dims_hash(dims)
  return dims.pixel_width .. "x" .. dims.pixel_height
end

local function update_window_config(window, is_full_screen, _pane)
  local now = monotonic_now()
  local dims = window:get_dimensions()
  local current_hash = dims_hash(dims)
  local state = resize_state_by_window[window]
  if not state then
    state = {
      last_resize_time = 0,
      last_dims_hash = "",
      last_is_full_screen = is_full_screen == true,
    }
    resize_state_by_window[window] = state
  end
  -- macOS Space/focus transitions can briefly report `is_full_screen=false`
  -- while the window is unfocused. If we immediately downgrade overrides
  -- in that transient frame, returning focus causes a visible padding jump.
  local effective_full_screen = is_full_screen
  if not effective_full_screen and state.last_is_full_screen then
    local ok_focused, focused = pcall(function()
      return window:is_focused()
    end)
    if ok_focused and not focused then
      effective_full_screen = true
    end
  end
  local overrides = window:get_config_overrides() or {}
  local needs_update = false

  -- Padding is now owned by the base config plus Rust layout policy.
  -- Keep overrides cleared so focus/resize transitions don't trigger a
  -- redundant config reload just to restate the default value.
  local padding_needs_update = overrides.window_padding ~= nil

  -- Top-tab fullscreen layout is now computed entirely in Rust so Space/app
  -- switches do not bounce through a second config reload that changes
  -- hide_tab_bar_if_only_one_tab or window_content_alignment.
  local tab_bar_needs_update = overrides.hide_tab_bar_if_only_one_tab ~= nil
  local alignment_needs_update = overrides.window_content_alignment ~= nil

  needs_update = padding_needs_update
    or tab_bar_needs_update
    or alignment_needs_update

  -- Skip update if dimensions changed rapidly (within 1 second) and state is stable
  -- This prevents padding flicker during fullscreen animation
  if current_hash ~= state.last_dims_hash then
    local time_since_last = now - state.last_resize_time
    if time_since_last < 1.0 and not needs_update then
      -- Rapid change detected, skip this update
      state.last_dims_hash = current_hash
      state.last_resize_time = now
      state.last_is_full_screen = effective_full_screen
      return
    end
    state.last_dims_hash = current_hash
    state.last_resize_time = now
  end

  if not needs_update then
    state.last_is_full_screen = effective_full_screen
    return
  end

  overrides.window_padding = nil
  overrides.hide_tab_bar_if_only_one_tab = nil
  overrides.window_content_alignment = nil

  state.last_is_full_screen = effective_full_screen
  window:set_config_overrides(overrides)
end

local function extract_path_from_cwd(cwd)
  if not cwd then
    return ''
  end

  local path = ''
  if type(cwd) == 'userdata' then
    -- pane:get_current_working_dir() returns a Url userdata (not a table).
    -- .file_path gives the already percent-decoded local path.
    path = (cwd.file_path or ''):gsub('/$', '')
    return path
  elseif type(cwd) == 'table' then
    path = cwd.file_path or cwd.path or tostring(cwd)
  else
    path = tostring(cwd)
  end

  path = path:gsub('^file://[^/]*', ''):gsub('/$', '')
  -- Decode URL-encoded characters (e.g., %E9%9F%B3%E4%B9%90 -> 音乐)
  path = url_decode(path)
  return path
end

local active_tab_cwd_cache = {}
-- os.time() returns integer wall-clock seconds; 1s granularity is fine for tab title throttle
local active_tab_cwd_refresh_interval = 1
local function now_secs()
  return os.time()
end
local runtime_cwd_startup_grace_secs = 3
local runtime_cwd_warmup_until_secs = now_secs() + runtime_cwd_startup_grace_secs

local home_dir = os.getenv("HOME")
local kaku_state_dir = home_dir and (home_dir .. "/.config/kaku") or nil
local lazygit_state_file = kaku_state_dir and (kaku_state_dir .. "/lazygit_state.json") or nil
local last_cwd_file = kaku_state_dir and (kaku_state_dir .. "/last_cwd") or nil
local last_saved_cwd = nil

local function save_last_cwd(path)
  if not last_cwd_file or not path or path == '' then return end
  if path == last_saved_cwd then return end
  local f = io.open(last_cwd_file, 'w')
  if f then
    f:write(path .. '\n')
    f:close()
    last_saved_cwd = path
  end
end

local function read_last_cwd()
  if not last_cwd_file then return nil end
  local f = io.open(last_cwd_file, 'r')
  if not f then return nil end
  local path = f:read('l')
  f:close()
  if not path or path == '' then return nil end
  return path
end
local lazygit_state_cache = nil
local lazygit_repo_probe_cache = {}
local lazygit_repo_probe_interval_secs = 5
local lazygit_command_probe = { value = nil, command = nil, checked_at = 0 }
local lazygit_command_probe_interval_secs = 30
local yazi_command_probe = { value = nil, command = nil, checked_at = 0 }
local yazi_command_probe_interval_secs = 30
local sshfs_command_probe = { value = nil, command = nil, checked_at = 0 }
local sshfs_command_probe_interval_secs = 30
local remote_files_mount_root = home_dir and (home_dir .. "/Library/Caches/dev.kaku/sshfs") or nil
local lazygit_hint_startup_grace_secs = 3
local lazygit_hint_warmup_until_secs = now_secs() + lazygit_hint_startup_grace_secs
local lazygit_hint_schedule_cooldown_secs = 8
local lazygit_hint_probe_state_by_pane = {}

local function trim_trailing_whitespace(value)
  if type(value) ~= "string" then
    return ""
  end
  return value:gsub("%s+$", "")
end

local function trim_surrounding_whitespace(value)
  if type(value) ~= "string" then
    return ""
  end
  return value:gsub("^%s+", ""):gsub("%s+$", "")
end

local function run_process(args)
  local ok, success, stdout, stderr = pcall(function()
    return wezterm.run_child_process(args)
  end)
  if not ok then
    return false, "", tostring(success or "")
  end
  return success, tostring(stdout or ""), tostring(stderr or "")
end

local function normalize_ai_summary(value, fallback)
  local text = trim_surrounding_whitespace(value or "")
  if text == "" then
    text = fallback or "Fix the command and retry."
  end

  text = text:gsub("[\r\n]+", " ")
  text = text:gsub("[\"']", "")
  text = text:gsub("%b()", "")
  text = text:gsub("^%s*[Tt]he%s+[Uu]ser%s+typed%s+", "")
  text = text:gsub("^%s*[Yy]ou%s+typed%s+", "")
  text = text:gsub("^%s*[Tt]his%s+command%s+", "Command ")
  text = text:gsub("^%s*[Cc]ommand%s+[Ff]ailed%s*:?%s*", "")
  text = text:gsub("^%s*[Ff]ailed%s*:?%s*", "")
  text = text:gsub("^%s*[Ee]rror%s*:?%s*", "")
  text = text:gsub("^%s*[Tt]he%s+command%s+", "Command ")
  text = text:gsub("%s+which%s+is%s+not%s+a%s+valid%s+", " is not a valid ")
  text = text:gsub("%s+is%s+not%s+recognized%s+", " is not recognized ")
  text = text:gsub("%s+maybe%s+the%s+user%s+meant.*$", "")
  text = text:gsub("%s+maybe%s+you%s+meant.*$", "")
  text = text:gsub("%s+maybe%s+.*$", "")
  text = text:gsub("%s+git%s+suggests.*$", "")
  text = text:gsub("%s+did%s+you%s+mean.*$", "")
  text = text:gsub("%s+", " ")
  text = trim_surrounding_whitespace(text)

  local sentence_end = text:find("[%.%!%?。！？]")
  if sentence_end and sentence_end > 0 then
    text = text:sub(1, sentence_end)
  end

  local max_chars = 72
  if #text > max_chars then
    local shortened = text:sub(1, max_chars)
    shortened = shortened:gsub("%s+[^%s]*$", "")
    if shortened == "" then
      shortened = text:sub(1, max_chars)
    end
    text = trim_surrounding_whitespace(shortened)
  end

  if text == "" then
    return "Fix the command and retry."
  end
  if not text:find("[%.%!%?。！？]$") then
    text = text .. "."
  end
  return text
end

local function strip_wrapping_quotes(value)
  if type(value) ~= "string" then
    return ""
  end
  local trimmed = trim_surrounding_whitespace(value)
  local first = trimmed:sub(1, 1)
  local last = trimmed:sub(-1)
  if #trimmed >= 2 and ((first == "'" and last == "'") or (first == '"' and last == '"')) then
    return trimmed:sub(2, -2)
  end
  return trimmed
end

local ai_fix_toml_path = kaku_state_dir and (kaku_state_dir .. "/assistant.toml") or nil
local ai_fix_file_settings = {}

local function strip_inline_toml_comment(line)
  if type(line) ~= "string" then
    return ""
  end

  local in_single = false
  local in_double = false
  local escaped = false
  local index = 1

  while index <= #line do
    local ch = line:sub(index, index)
    if in_double then
      if escaped then
        escaped = false
      elseif ch == "\\" then
        escaped = true
      elseif ch == '"' then
        in_double = false
      end
    elseif in_single then
      if ch == "'" then
        in_single = false
      end
    else
      if ch == '"' then
        in_double = true
      elseif ch == "'" then
        in_single = true
      elseif ch == "#" then
        return trim_surrounding_whitespace(line:sub(1, index - 1))
      end
    end
    index = index + 1
  end

  return trim_surrounding_whitespace(line)
end

local function parse_ai_toml_setting_value(raw_value)
  local value = trim_surrounding_whitespace(raw_value or "")
  if value == "" then
    return nil
  end

  if value:sub(1, 1) == "[" and value:sub(-1) == "]" then
    local items = {}
    local content = trim_surrounding_whitespace(value:sub(2, -2))
    if content ~= "" then
      for part in content:gmatch("[^,]+") do
        local item = strip_wrapping_quotes(part)
        if item ~= "" then
          items[#items + 1] = item
        end
      end
    end
    return table.concat(items, ",")
  end

  if value == "true" then
    return true
  end
  if value == "false" then
    return false
  end

  local number_value = tonumber(value)
  if number_value ~= nil then
    return number_value
  end

  return strip_wrapping_quotes(value)
end

local function parse_ai_toml_custom_headers(raw_value)
  local value = trim_surrounding_whitespace(raw_value or "")
  if value == "" then
    return {}
  end

  if value:sub(1, 1) == "[" and value:sub(-1) == "]" then
    local headers = {}
    local content = trim_surrounding_whitespace(value:sub(2, -2))
    if content ~= "" then
      local token = {}
      local in_double = false
      local in_single = false
      local escaped = false

      local function flush_token()
        local part = trim_surrounding_whitespace(table.concat(token))
        token = {}
        if part == "" then
          return
        end
        local item = strip_wrapping_quotes(part)
        if item ~= "" then
          headers[#headers + 1] = item
        end
      end

      local i = 1
      while i <= #content do
        local ch = content:sub(i, i)

        if in_double then
          token[#token + 1] = ch
          if escaped then
            escaped = false
          elseif ch == "\\" then
            escaped = true
          elseif ch == '"' then
            in_double = false
          end
        elseif in_single then
          token[#token + 1] = ch
          if ch == "'" then
            in_single = false
          end
        else
          if ch == "," then
            flush_token()
          else
            token[#token + 1] = ch
            if ch == '"' then
              in_double = true
            elseif ch == "'" then
              in_single = true
            end
          end
        end

        i = i + 1
      end

      flush_token()
    end
    return headers
  end

  local single = strip_wrapping_quotes(value)
  if single == "" then
    return {}
  end
  return { single }
end

local function load_ai_fix_file_settings()
  local settings = {}
  if not ai_fix_toml_path or ai_fix_toml_path == "" then
    return settings
  end

  local file = io.open(ai_fix_toml_path, "r")
  if not file then
    return settings
  end

  for raw_line in file:lines() do
    local line = strip_inline_toml_comment(raw_line)
    if line ~= "" then
      if not line:match("^%s*%[") then
        local key, raw_value = line:match("^%s*([%w_%-]+)%s*=%s*(.-)%s*$")
        if key and raw_value then
          local parsed = nil
          if key == "custom_headers" then
            parsed = parse_ai_toml_custom_headers(raw_value)
          else
            parsed = parse_ai_toml_setting_value(raw_value)
          end
          if parsed ~= nil then
            settings[key] = parsed
          end
        end
      end
    end
  end

  file:close()
  return settings
end

local function stringify_ai_setting_value(value)
  if value == nil then
    return nil
  end
  if type(value) == "boolean" then
    return value and "1" or "0"
  end
  if type(value) == "number" then
    return tostring(value)
  end
  if type(value) == "string" then
    local normalized = trim_surrounding_whitespace(value)
    if normalized == "" then
      return nil
    end
    if normalized == "true" then
      return "1"
    end
    if normalized == "false" then
      return "0"
    end
    return normalized
  end
  return nil
end

local function read_ai_setting(file_key, default_value)
  local value = stringify_ai_setting_value(ai_fix_file_settings[file_key])
  if not value or value == "" then
    return default_value
  end
  return value
end

local function parse_ai_custom_header_entry(raw_header)
  if type(raw_header) ~= "string" then
    return nil, nil
  end

  local trimmed = trim_surrounding_whitespace(raw_header)
  if trimmed == "" then
    return nil, nil
  end

  local colon_at = trimmed:find(":", 1, true)
  if not colon_at or colon_at <= 1 then
    return nil, nil
  end

  local name = trim_surrounding_whitespace(trimmed:sub(1, colon_at - 1))
  local value = trim_surrounding_whitespace(trimmed:sub(colon_at + 1))
  if name == "" or value == "" then
    return nil, nil
  end

  return name .. ": " .. value, string.lower(name)
end

local function read_ai_custom_headers(file_key)
  local raw_headers = ai_fix_file_settings[file_key]
  if type(raw_headers) ~= "table" then
    return {}
  end

  local headers = {}
  local seen = {
    ["authorization"] = true,
    ["content-type"] = true,
  }

  for _, raw in ipairs(raw_headers) do
    local parsed, name_key = parse_ai_custom_header_entry(raw)
    if parsed and name_key and not seen[name_key] then
      seen[name_key] = true
      headers[#headers + 1] = parsed
    end
  end

  return headers
end

-- Detect if the foreground process is a shell.
-- Returns false for interactive programs like claude, vim, ssh.
local function is_shell_foreground(pane)
  if not pane then
    return false
  end

  local ok, proc = pcall(function()
    return pane:get_foreground_process_name()
  end)
  if not ok or type(proc) ~= "string" or proc == "" then
    return false
  end

  -- Normalize full executable paths (e.g., /bin/zsh) and login shell names (e.g., -zsh).
  local name = basename(proc:lower()):gsub("^%-", "")
  local shells = { zsh = true, bash = true, fish = true, sh = true, dash = true, ksh = true, tcsh = true, csh = true }
  return shells[name] == true
end

-- Keep cold startup fast: parse assistant.toml lazily only when AI fix is needed.
local ai_fix_enabled = true
local ai_fix_api_base_url = "https://api.vivgrid.com/v1"
local ai_fix_api_key = nil
local ai_fix_model = "DeepSeek-V3.2"
local ai_fix_custom_headers = {}
local ai_fix_timeout_secs = 12
local ai_fix_debug_enabled = false
local ai_fix_state_by_pane = {}
local ai_fix_poll_interval_secs = 0.2
local ai_fix_poll_deadline_secs = ai_fix_timeout_secs + 4
local ai_fix_jobs_dir = kaku_state_dir and (kaku_state_dir .. "/ai_jobs") or "/tmp"
local ai_fix_job_counter = 0

local function ai_debug_log(message)
  if not ai_fix_debug_enabled then
    return
  end
  local now = os.date("!%Y-%m-%dT%H:%M:%SZ")
  local file = io.open("/tmp/kaku_ai_debug.log", "a")
  if not file then
    return
  end
  file:write(now .. " " .. tostring(message) .. "\n")
  file:close()
end

local function refresh_ai_fix_settings()
  ai_fix_file_settings = load_ai_fix_file_settings()
  ai_fix_enabled = read_ai_setting("enabled", ai_fix_enabled and "1" or "0") ~= "0"
  ai_fix_api_base_url = read_ai_setting("base_url", ai_fix_api_base_url)
  ai_fix_api_key = read_ai_setting("api_key", ai_fix_api_key)
  ai_fix_model = read_ai_setting("model", ai_fix_model)
  ai_fix_custom_headers = read_ai_custom_headers("custom_headers")
end

local function detect_git_branch(path)
  if not path or path == "" then
    return ""
  end

  local ok, stdout = wezterm.run_child_process({
    "git",
    "-C",
    path,
    "rev-parse",
    "--abbrev-ref",
    "HEAD",
  })
  if not ok then
    return ""
  end
  return trim_trailing_whitespace(stdout)
end

local function build_ai_fix_messages(failed_command, exit_code, cwd, git_branch)
  local context = {
    "Command: " .. failed_command,
    "Exit code: " .. tostring(exit_code),
    "Working directory: " .. (cwd ~= "" and cwd or "(unknown)"),
  }

  if git_branch ~= "" then
    context[#context + 1] = "Git branch: " .. git_branch
  end

  return {
    {
      role = "system",
      content = "You are a shell troubleshooting assistant. Output English only and return exactly one JSON object with keys summary, command, why, confidence. Do not use markdown or code fences. summary must be one concise sentence <= 72 chars and must not contain parentheses. command must be a single direct fix command without commentary. Avoid diagnostic-only commands, alias probing, and placeholders. Never use aliases like ll. If you are not confident about a direct fix, set command to an empty string.",
    },
    {
      role = "user",
      content = table.concat(context, "\n"),
    },
  }
end

local function ai_fix_endpoint()
  return trim_trailing_whitespace(ai_fix_api_base_url):gsub("/+$", "") .. "/chat/completions"
end

local function ai_fix_curl_header_args()
  local args = {
    "-H",
    "Authorization: Bearer " .. ai_fix_api_key,
    "-H",
    "Content-Type: application/json",
  }

  for _, header in ipairs(ai_fix_custom_headers) do
    args[#args + 1] = "-H"
    args[#args + 1] = header
  end

  return args
end

local function encode_ai_fix_payload(model, messages)
  local payload_ok, payload = pcall(wezterm.json_encode, {
    model = model,
    messages = messages,
    stream = false,
  })
  if not payload_ok or type(payload) ~= "string" or payload == "" then
    return nil, "failed to encode request payload"
  end
  return payload
end

local function next_ai_fix_job_id()
  ai_fix_job_counter = ai_fix_job_counter + 1
  return tostring(now_secs()) .. "-" .. tostring(ai_fix_job_counter)
end

local function ensure_ai_fix_jobs_dir()
  if not ai_fix_jobs_dir or ai_fix_jobs_dir == "" then
    return false
  end
  os.execute(string.format("mkdir -p %q", ai_fix_jobs_dir))
  return true
end

local function write_text_file(path, content)
  local file = io.open(path, "w")
  if not file then
    return false
  end
  file:write(content or "")
  file:close()
  return true
end

local function read_text_file(path)
  local file = io.open(path, "r")
  if not file then
    return nil
  end
  local content = file:read("*a")
  file:close()
  return content
end

local function cleanup_ai_fix_job_files(job)
  if not job or not job.paths then
    return
  end
  os.remove(job.paths.request_path)
  os.remove(job.paths.response_path)
  os.remove(job.paths.stderr_path)
  os.remove(job.paths.status_path)
end

local function start_ai_fix_background_job(payload)
  if not ensure_ai_fix_jobs_dir() then
    return nil, "state directory unavailable"
  end

  local job_id = next_ai_fix_job_id()
  local base_path = ai_fix_jobs_dir .. "/ai_fix_" .. job_id
  local job = {
    id = job_id,
    started_at = now_secs(),
    paths = {
      request_path = base_path .. ".request.json",
      response_path = base_path .. ".response.json",
      stderr_path = base_path .. ".stderr.log",
      status_path = base_path .. ".status",
    },
  }

  if not write_text_file(job.paths.request_path, payload) then
    return nil, "failed to write request payload"
  end

  local curl_header_args = ai_fix_curl_header_args()

  local script = [[
status=0
connect_timeout="$1"
max_time="$2"
url="$3"
request_path="$4"
response_path="$5"
stderr_path="$6"
status_path="$7"
shift 7

set -- -sS --fail --connect-timeout "$connect_timeout" --max-time "$max_time" "$url" "$@" \
  --data-binary "@$request_path" \
  -o "$response_path" \
  --stderr "$stderr_path"

curl "$@"
status=$?
printf '%s' "$status" > "$status_path"
]]
  local launched_ok, launch_err = pcall(function()
    local launch_args = {
      "sh",
      "-c",
      script,
      "kaku-ai-fix",
      "3",
      tostring(ai_fix_timeout_secs),
      ai_fix_endpoint(),
      job.paths.request_path,
      job.paths.response_path,
      job.paths.stderr_path,
      job.paths.status_path,
    }
    for _, arg in ipairs(curl_header_args) do
      launch_args[#launch_args + 1] = arg
    end
    wezterm.background_child_process(launch_args)
  end)

  if not launched_ok then
    cleanup_ai_fix_job_files(job)
    return nil, trim_surrounding_whitespace(tostring(launch_err or "failed to launch request"))
  end

  return job
end

local function extract_assistant_message_content(response)
  if type(response) ~= "table" then
    return nil
  end
  local choices = response.choices
  if type(choices) ~= "table" or #choices == 0 then
    return nil
  end
  local message = choices[1] and choices[1].message
  if type(message) ~= "table" then
    return nil
  end

  local content = message.content
  if type(content) == "string" then
    return content
  end
  if type(content) ~= "table" then
    return nil
  end

  local chunks = {}
  for _, item in ipairs(content) do
    if type(item) == "table" and type(item.text) == "string" then
      chunks[#chunks + 1] = item.text
    elseif type(item) == "string" then
      chunks[#chunks + 1] = item
    end
  end
  if #chunks == 0 then
    return nil
  end
  return table.concat(chunks, "\n")
end

local function parse_json_object_from_text(value)
  if type(value) ~= "string" then
    return nil
  end

  local text = trim_surrounding_whitespace(value)
  local direct_ok, parsed = pcall(wezterm.json_parse, text)
  if direct_ok and type(parsed) == "table" then
    return parsed
  end

  local first_brace = text:find("{", 1, true)
  local last_brace = nil
  for idx = #text, 1, -1 do
    if text:sub(idx, idx) == "}" then
      last_brace = idx
      break
    end
  end
  if not first_brace or not last_brace or last_brace < first_brace then
    return nil
  end

  local candidate = text:sub(first_brace, last_brace)
  local nested_ok, nested = pcall(wezterm.json_parse, candidate)
  if nested_ok and type(nested) == "table" then
    return nested
  end
  return nil
end

local function sanitize_suggested_command(value)
  if type(value) ~= "string" then
    return ""
  end

  local command = trim_surrounding_whitespace(value)
  command = command:gsub("\r", "")
  command = command:gsub("^```[%w_-]*\n?", "")
  command = command:gsub("\n```$", "")
  command = trim_surrounding_whitespace(command)

  local first_line = command:match("([^\n]+)") or command
  first_line = trim_surrounding_whitespace(first_line)
  first_line = first_line:gsub("^%$%s*", "")
  return first_line
end

local function is_dangerous_command(command)
  if type(command) ~= "string" or command == "" then
    return false
  end
  local lower = trim_surrounding_whitespace(command:lower())
  if lower == "" then
    return false
  end

  if lower:match("^:%(%){:%|:&};:$") then
    return true
  end

  if lower:match("^%s*sudo%s+.*%srm%s+.-%-%w*r%w*f%w*") or lower:match("^%s*sudo%s+.*%srm%s+.-%-%w*f%w*r%w*") then
    return true
  end

  local normalized = lower:gsub("^%s*sudo%s+", "")
  if normalized:match("^%s*rm%s+.-%-%w*r%w*f%w*") or normalized:match("^%s*rm%s+.-%-%w*f%w*r%w*") then
    return true
  end

  local patterns = {
    "^%s*mkfs",
    "^%s*dd%s+if=",
    "^%s*shutdown",
    "^%s*reboot",
    "^%s*poweroff",
    "^%s*git%s+reset%s+%-%-hard",
    "^%s*git%s+clean%s+%-[%w]*f[%w]*d",
  }
  for _, pattern in ipairs(patterns) do
    if normalized:match(pattern) then
      return true
    end
  end
  return false
end

local function is_non_actionable_ai_command(command)
  local normalized = trim_surrounding_whitespace(command or "")
  if normalized == "" then
    return false
  end
  local lower = normalized:lower()
  local patterns = {
    "^%s*type%s+",
    "^%s*which%s+",
    "^%s*command%s+%-v%s+",
    "^%s*ll%s*$",
    "^%s*ll%s+",
    "%|%|",
    "&&%s*echo",
    ";%s*echo",
  }
  for _, pattern in ipairs(patterns) do
    if lower:match(pattern) then
      return true
    end
  end
  return false
end

local function should_skip_ai_fix_for_failed_command(failed_command, exit_code)
  local normalized = trim_surrounding_whitespace(failed_command or "")
  if normalized == "" then
    return true
  end

  local lower = normalized:lower()

  -- Treat explicit help/usage intent as non-errors for AI suggestions.
  if lower:match("%-%-help") or lower:match("%s%-h%s*$") or lower:match("%f[%w]help%f[%W]") then
    return true
  end

  -- Some package managers print usage and may return non-zero for bare invocation.
  -- Avoid noisy suggestions for these common entry commands.
  local bare_cmd = lower:match("^([%w%._%-%/]+)%s*$")
  if bare_cmd and (
      bare_cmd == "tnpm"
      or bare_cmd == "npm"
      or bare_cmd == "pnpm"
      or bare_cmd == "yarn"
      or bare_cmd == "pip"
      or bare_cmd == "pip3"
    ) then
    return true
  end

  -- git pull conflicts are an expected workflow state; avoid auto-fix noise.
  if lower:match("^git%s+pull(%s|$)") and exit_code ~= 0 then
    return true
  end

  return false
end

local function parse_ai_fix_result(content)
  local parsed = parse_json_object_from_text(content or "")
  if type(parsed) == "table" then
    local summary = normalize_ai_summary(tostring(parsed.summary or ""), "Fix the command and retry.")
    local command = sanitize_suggested_command(parsed.command or "")
    local why = trim_surrounding_whitespace(tostring(parsed.why or ""))
    local confidence = tonumber(parsed.confidence) or 0
    return {
      summary = summary,
      command = command,
      why = why,
      confidence = confidence,
    }
  end

  local first_line = normalize_ai_summary((content or ""):match("([^\n]+)") or "", "Fix the command and retry.")
  return {
    summary = first_line,
    command = "",
    why = "",
    confidence = 0,
  }
end

local function parse_ai_fix_response(stdout)
  local parsed_ok, response = pcall(wezterm.json_parse, stdout or "")
  if not parsed_ok or type(response) ~= "table" then
    return nil, "invalid json response"
  end
  local content = extract_assistant_message_content(response)
  if not content or content == "" then
    return nil, "empty model response"
  end

  local result = parse_ai_fix_result(content)
  if result.command ~= "" and is_non_actionable_ai_command(result.command) then
    result.command = ""
  end
  return result
end

local function inject_ai_notice(pane, headline, detail, suggested_command)
  if not pane then
    return
  end

  local summary = trim_surrounding_whitespace(headline or "Update")
  local normalized_detail = trim_surrounding_whitespace(detail or "")
  if normalized_detail ~= "" and summary ~= "" then
    summary = summary .. ": " .. normalized_detail
  elseif normalized_detail ~= "" then
    summary = normalized_detail
  end

  local cmd = sanitize_suggested_command(suggested_command or "")
  local summary_line = "\27[38;5;141m╭─ Kaku Assistant\27[0m  \27[1m" .. summary .. "\27[0m"
  local command_line = ""
  if cmd ~= "" then
    command_line = "\27[38;5;141m╰─\27[0m " .. cmd .. "    \27[38;5;244mCmd+Shift+E\27[0m"
  else
    command_line = "\27[38;5;141m╰─\27[0m \27[38;5;244mNo safe command suggested\27[0m"
  end

  local output = "\r\n" .. summary_line .. "\r\n" .. command_line
  pcall(function()
    pane:inject_output(output)
  end)
end

local function finalize_shell_line(pane)
  if not pane then
    return
  end
  -- Ensure prompt returns to an input-ready state without manual Enter.
  pcall(function()
    pane:send_text("\n")
  end)
end

local function inject_ai_status(pane, message)
  if not pane then
    return
  end

  local summary = normalize_ai_summary(message or "", "Checking this error now.")
  local line = "\27[38;5;141m╰─ Kaku Assistant\27[0m  \27[38;5;244m" .. summary .. "\27[0m"
  local output = "\r\n" .. line .. "\r\n"
  pcall(function()
    pane:inject_output(output)
  end)
end

local function inject_ai_status_and_finalize(pane, message)
  inject_ai_status(pane, message)
  finalize_shell_line(pane)
end

local function show_ai_loading_toast(window, pane)
  if not window or not pane then
    return
  end
  pcall(function()
    window:perform_action(wezterm.action.EmitEvent("kaku-toast-ai-analyzing"), pane)
  end)
end

local function clear_ai_fix_suggestion_state(pane_state)
  if not pane_state then
    return
  end
  pane_state.suggestion = nil
  pane_state.generated_at = nil
end

local function poll_ai_fix_job(window, pane, pane_id, job, failed_command, exit_code)
  local pane_state = ai_fix_state_by_pane[pane_id]
  if not pane_state or pane_state.pending_job_id ~= job.id then
    cleanup_ai_fix_job_files(job)
    return
  end

  local status_text = read_text_file(job.paths.status_path)
  local status_value = trim_surrounding_whitespace(status_text or "")
  if status_value == "" then
    if (now_secs() - job.started_at) >= ai_fix_poll_deadline_secs then
      pane_state.inflight = false
      pane_state.pending_job_id = nil
      clear_ai_fix_suggestion_state(pane_state)
      cleanup_ai_fix_job_files(job)
      ai_debug_log("ai_fix_job timeout pane_id=" .. pane_id)
      inject_ai_status_and_finalize(pane, "Could not analyze this error right now.")
      return
    end

    wezterm.time.call_after(ai_fix_poll_interval_secs, function()
      poll_ai_fix_job(window, pane, pane_id, job, failed_command, exit_code)
    end)
    return
  end

  local status_code = tonumber(status_value)
  local stdout = read_text_file(job.paths.response_path) or ""
  local stderr = read_text_file(job.paths.stderr_path) or ""
  cleanup_ai_fix_job_files(job)

  pane_state.inflight = false
  pane_state.pending_job_id = nil

  if status_code ~= 0 then
    clear_ai_fix_suggestion_state(pane_state)
    ai_debug_log("ai_fix_job failed pane_id=" .. pane_id .. " status=" .. tostring(status_code) .. " err=" .. tostring(stderr))
    inject_ai_status_and_finalize(pane, "Could not analyze this error right now.")
    return
  end

  local result, parse_err = parse_ai_fix_response(stdout)
  if not result then
    clear_ai_fix_suggestion_state(pane_state)
    ai_debug_log("ai_fix_job invalid_response pane_id=" .. pane_id .. " err=" .. tostring(parse_err))
    inject_ai_status_and_finalize(pane, "Could not analyze this error right now.")
    return
  end

  result.model = ai_fix_model
  pane_state.suggestion = result
  pane_state.failed_command = failed_command
  pane_state.exit_code = exit_code
  pane_state.generated_at = now_secs()

  local command = sanitize_suggested_command(result.command or "")
  if command == "" then
    local summary = normalize_ai_summary(result.summary or "", "No quick fix command found.")
    inject_ai_status_and_finalize(pane, summary)
    return
  end

  inject_ai_notice(
    pane,
    normalize_ai_summary(result.summary or "", "Fix the command and retry."),
    "",
    command
  )
  finalize_shell_line(pane)
end

local function request_ai_fix_async(window, pane, pane_id, failed_command, exit_code, cwd, git_branch)
  local messages = build_ai_fix_messages(failed_command, exit_code, cwd, git_branch)
  local payload, payload_err = encode_ai_fix_payload(ai_fix_model, messages)
  if not payload then
    return nil, payload_err or "failed to encode request payload"
  end

  local job, job_err = start_ai_fix_background_job(payload)
  if not job then
    return nil, job_err or "failed to start background request"
  end

  local pane_state = ai_fix_state_by_pane[pane_id]
  if pane_state then
    pane_state.pending_job_id = job.id
  end
  ai_debug_log("ai_fix_job started pane_id=" .. pane_id .. " job_id=" .. job.id)

  wezterm.time.call_after(ai_fix_poll_interval_secs, function()
    poll_ai_fix_job(window, pane, pane_id, job, failed_command, exit_code)
  end)
  return true
end

local function ensure_kaku_state_dir()
  if not kaku_state_dir or kaku_state_dir == "" then
    return
  end
  os.execute(string.format("mkdir -p %q", kaku_state_dir))
end

local function load_lazygit_state()
  if lazygit_state_cache then
    return lazygit_state_cache
  end

  local state = { repos = {} }
  if not lazygit_state_file then
    lazygit_state_cache = state
    return state
  end

  local file = io.open(lazygit_state_file, "r")
  if file then
    local raw = file:read("*all")
    file:close()
    if raw and raw ~= "" then
      local ok, parsed = pcall(wezterm.json_parse, raw)
      if ok and type(parsed) == "table" then
        state = parsed
      end
    end
  end

  if type(state.repos) ~= "table" then
    state.repos = {}
  end

  lazygit_state_cache = state
  return state
end

local function save_lazygit_state()
  if not lazygit_state_file then
    return
  end

  ensure_kaku_state_dir()
  local state = load_lazygit_state()
  local ok, encoded = pcall(wezterm.json_encode, state)
  if not ok or type(encoded) ~= "string" or encoded == "" then
    return
  end

  local file = io.open(lazygit_state_file, "w")
  if not file then
    return
  end
  file:write(encoded .. "\n")
  file:close()
end

local function get_lazygit_repo_flags(repo_root)
  local repos = load_lazygit_state().repos
  local flags = repos[repo_root]
  if type(flags) ~= "table" then
    flags = { hinted = false, used = false }
    repos[repo_root] = flags
  end
  flags.hinted = flags.hinted == true
  flags.used = flags.used == true
  return flags
end

local function mark_repo_lazygit_hinted(repo_root)
  if not repo_root or repo_root == "" then
    return
  end
  local flags = get_lazygit_repo_flags(repo_root)
  if flags.hinted then
    return
  end
  flags.hinted = true
  save_lazygit_state()
end

local function mark_repo_lazygit_used(repo_root)
  if not repo_root or repo_root == "" then
    return
  end
  local flags = get_lazygit_repo_flags(repo_root)
  if flags.used then
    return
  end
  flags.used = true
  save_lazygit_state()
end

local function pane_cwd(pane)
  if not pane then
    return ""
  end

  local ok, runtime_cwd = pcall(function()
    return pane:get_current_working_dir()
  end)
  if ok and runtime_cwd then
    local path = extract_path_from_cwd(runtime_cwd)
    if path ~= "" then
      return path
    end
  end

  return extract_path_from_cwd(pane.current_working_dir)
end

local function detect_git_repo_root(path)
  if not path or path == "" then
    return nil
  end

  local ok, stdout = wezterm.run_child_process({
    "git",
    "-C",
    path,
    "rev-parse",
    "--show-toplevel",
  })
  if not ok then
    return nil
  end

  local repo_root = trim_trailing_whitespace(stdout)
  if repo_root == "" then
    return nil
  end
  return repo_root
end

local function repo_has_pending_changes(repo_root)
  local ok, stdout = wezterm.run_child_process({
    "git",
    "-C",
    repo_root,
    "status",
    "--porcelain",
    "--untracked-files=no",
    "--no-optional-locks",
  })
  if not ok then
    return false
  end
  return trim_trailing_whitespace(stdout) ~= ""
end

local function git_repo_context(path)
  local now = now_secs()
  local cached = lazygit_repo_probe_cache[path]
  if cached and (now - cached.checked_at) < lazygit_repo_probe_interval_secs then
    return cached.repo_root, cached.has_changes
  end

  local repo_root = detect_git_repo_root(path)
  local has_changes = false
  if repo_root then
    has_changes = repo_has_pending_changes(repo_root)
  end

  lazygit_repo_probe_cache[path] = {
    checked_at = now,
    repo_root = repo_root,
    has_changes = has_changes,
  }

  return repo_root, has_changes
end

local function resolve_lazygit_command()
  local now = now_secs()
  local cached_value = lazygit_command_probe.value
  if cached_value ~= nil then
    local age = now - lazygit_command_probe.checked_at
    if cached_value or age < lazygit_command_probe_interval_secs then
      return lazygit_command_probe.command
    end
  end

  local candidates = {
    "lazygit",
    "/opt/homebrew/bin/lazygit",
    "/usr/local/bin/lazygit",
  }
  local resolved = nil
  for _, cmd in ipairs(candidates) do
    local call_ok, run_result = pcall(function()
      return select(1, wezterm.run_child_process({ cmd, "--version" }))
    end)
    if call_ok and run_result then
      resolved = cmd
      break
    end
  end

  lazygit_command_probe.value = resolved ~= nil
  lazygit_command_probe.command = resolved
  lazygit_command_probe.checked_at = now
  return resolved
end

local function is_lazygit_installed()
  return resolve_lazygit_command() ~= nil
end

local function resolve_yazi_command()
  local now = now_secs()
  local cached_value = yazi_command_probe.value
  if cached_value ~= nil then
    local age = now - yazi_command_probe.checked_at
    if cached_value or age < yazi_command_probe_interval_secs then
      return yazi_command_probe.command
    end
  end

  local home = os.getenv("HOME") or ""
  local candidates = { "yazi", "/opt/homebrew/bin/yazi", "/usr/local/bin/yazi" }
  if home ~= "" then
    table.insert(candidates, 1, home .. "/.config/kaku/zsh/bin/yazi")
    table.insert(candidates, 1, home .. "/.config/kaku/fish/bin/yazi")
  end
  local resolved = nil
  for _, cmd in ipairs(candidates) do
    local call_ok, run_result = pcall(function()
      return select(1, wezterm.run_child_process({ cmd, "--version" }))
    end)
    if call_ok and run_result then
      resolved = cmd
      break
    end
  end

  yazi_command_probe.value = resolved ~= nil
  yazi_command_probe.command = resolved
  yazi_command_probe.checked_at = now
  return resolved
end

local kaku_yazi_theme_marker_start = "# ===== Kaku Yazi Flavor (managed) ====="
local kaku_yazi_theme_marker_end = "# ===== End Kaku Yazi Flavor (managed) ====="

local function current_yazi_flavor(window)
  local overrides = window and window:get_config_overrides() or {}
  local scheme = resolve_kaku_color_scheme(overrides.color_scheme or config.color_scheme)
  return scheme == 'Kaku Light' and 'kaku-light' or 'kaku-dark'
end

local function strip_managed_yazi_theme_block(content)
  local lines = {}
  local skipping = false

  for line in (content .. "\n"):gmatch("(.-)\n") do
    if line == kaku_yazi_theme_marker_start then
      skipping = true
    elseif line == kaku_yazi_theme_marker_end then
      skipping = false
    elseif not skipping then
      table.insert(lines, line)
    end
  end

  return table.concat(lines, "\n")
end

local function build_managed_yazi_theme_block(flavor)
  return table.concat({
    kaku_yazi_theme_marker_start,
    "[flavor]",
    string.format('dark = "%s"', flavor),
    string.format('light = "%s"', flavor),
    kaku_yazi_theme_marker_end,
  }, "\n")
end

local function is_legacy_kaku_yazi_theme(content)
  if content:find("# Kaku%-aligned theme for Yazi 26%.x", 1) then
    return true
  end

  local normalized = content
      :gsub("[ \t]+\n", "\n")
      :gsub("^%s+", "")
      :gsub("%s+$", "")
      :gsub("\n+", "\n")

  return normalized == table.concat({
    "[mgr]",
    'border_symbol = "│"',
    'border_style = { fg = "#555555" }',
    "[indicator]",
    'padding = { open = "", close = "" }',
  }, "\n")
end

local function sync_managed_yazi_theme(window)
  local home = os.getenv("HOME")
  if not home or home == "" then
    return
  end

  local yazi_dir = home .. "/.config/yazi"
  local theme_path = yazi_dir .. "/theme.toml"
  local flavor = current_yazi_flavor(window)
  local managed_block = build_managed_yazi_theme_block(flavor)

  os.execute(string.format("mkdir -p %q", yazi_dir))

  local existing = ""
  local theme_file = io.open(theme_path, "r")
  if theme_file then
    existing = theme_file:read("*all") or ""
    theme_file:close()
  end

  if not is_legacy_kaku_yazi_theme(existing) and existing:find(managed_block, 1, true) then
    return
  end

  local updated
  if existing == "" or is_legacy_kaku_yazi_theme(existing) then
    updated = table.concat({
      '"$schema" = "https://yazi-rs.github.io/schemas/theme.json"',
      "",
      "# Kaku manages the [flavor] section below so Yazi matches the current Kaku theme.",
      managed_block,
      "",
    }, "\n")
  else
    local has_user_flavor = ("\n" .. existing .. "\n"):find("\n%s*%[flavor%]%s*\n") ~= nil
    local has_managed = existing:find(kaku_yazi_theme_marker_start, 1, true) ~= nil
    if has_user_flavor and not has_managed then
      return
    end

    updated = strip_managed_yazi_theme_block(existing)
    if updated ~= "" and not updated:match("\n$") then
      updated = updated .. "\n"
    end
    updated = updated .. "\n" .. managed_block .. "\n"
  end

  write_text_file(theme_path, updated)
end

local function resolve_sshfs_command()
  local now = now_secs()
  local cached_value = sshfs_command_probe.value
  if cached_value ~= nil then
    local age = now - sshfs_command_probe.checked_at
    if cached_value or age < sshfs_command_probe_interval_secs then
      return sshfs_command_probe.command
    end
  end

  local candidates = {
    "sshfs",
    "/opt/homebrew/bin/sshfs",
    "/usr/local/bin/sshfs",
  }
  local resolved = nil
  for _, cmd in ipairs(candidates) do
    local call_ok, run_result = pcall(function()
      return select(1, wezterm.run_child_process({ cmd, "--version" }))
    end)
    if call_ok and run_result then
      resolved = cmd
      break
    end
  end

  sshfs_command_probe.value = resolved ~= nil
  sshfs_command_probe.command = resolved
  sshfs_command_probe.checked_at = now
  return resolved
end

local function pane_is_lazygit(pane)
  if not pane then
    return false
  end

  local ok, proc = pcall(function()
    return pane:get_foreground_process_name()
  end)
  if not ok or type(proc) ~= "string" or proc == "" then
    return false
  end

  return basename(proc) == "lazygit"
end

local function resolve_active_pane(window, pane)
  if pane then
    return pane
  end
  if not window then
    return nil
  end

  local ok_tab, tab = pcall(function()
    return window:active_tab()
  end)
  if ok_tab and tab then
    local ok_pane, active_pane = pcall(function()
      return tab:active_pane()
    end)
    if ok_pane then
      return active_pane
    end
  end

  return nil
end

local function show_lazygit_toast(window, pane, event_name)
  if not window then
    return
  end
  pcall(function()
    window:perform_action(wezterm.action.EmitEvent(event_name), pane)
  end)
end

local function show_yazi_toast(window, pane, event_name)
  show_lazygit_toast(window, pane, event_name)
end

local function show_remote_files_toast(window, message, timeout_ms)
  if not window then
    return
  end
  pcall(function()
    window:toast_notification("Remote Files", message, nil, timeout_ms or 4000)
  end)
end

local function extract_ssh_domain_target(domain_name)
  if type(domain_name) ~= "string" then
    return nil
  end

  local target = domain_name:match("^SSH:(.+)$")
  if not target then
    target = domain_name:match("^SSHMUX:(.+)$")
  end
  target = trim_surrounding_whitespace(target or "")
  if target == "" then
    return nil
  end
  return target
end

local function sanitize_mount_component(value)
  local component = trim_surrounding_whitespace(tostring(value or ""))
  component = component:gsub("[^%w%._%-@]", "_")
  if component == "" then
    return "remote"
  end
  return component
end

local function ssh_option_needs_value(token)
  if type(token) ~= "string" or #token ~= 2 or token:sub(1, 1) ~= "-" then
    return false
  end

  local option = token:sub(2, 2)
  return option == "B"
    or option == "b"
    or option == "c"
    or option == "D"
    or option == "E"
    or option == "e"
    or option == "F"
    or option == "I"
    or option == "i"
    or option == "J"
    or option == "L"
    or option == "l"
    or option == "m"
    or option == "O"
    or option == "o"
    or option == "p"
    or option == "Q"
    or option == "R"
    or option == "S"
    or option == "W"
    or option == "w"
end

local function normalize_ssh_target(target)
  local host = trim_surrounding_whitespace(target or "")
  if host == "" then
    return nil
  end

  local at = host:match(".*@(.+)$")
  if at and at ~= "" then
    host = at
  end

  local bracketed = host:match("^%[([^%]]+)%]")
  if bracketed and bracketed ~= "" then
    return bracketed
  end

  local maybe_host, maybe_port = host:match("^(.*):(%d+)$")
  if maybe_host and maybe_host ~= "" and maybe_port and maybe_port ~= "" then
    host = maybe_host
  end

  if host == "" then
    return nil
  end
  return host
end

local function ssh_target_from_tokens(tokens)
  if type(tokens) ~= "table" or #tokens == 0 then
    return nil
  end

  local command = basename(tostring(tokens[1] or ""))
  if command ~= "ssh" then
    return nil
  end

  local expect_value = false
  for i = 2, #tokens do
    local token = tostring(tokens[i] or "")
    if expect_value then
      expect_value = false
    elseif token == "--" then
      return nil
    elseif token:sub(1, 1) == "-" then
      expect_value = ssh_option_needs_value(token)
    else
      return normalize_ssh_target(token)
    end
  end

  return nil
end

local function shell_split(command)
  local tokens = {}
  for token in tostring(command or ""):gmatch("%S+") do
    tokens[#tokens + 1] = token
  end
  return tokens
end

local function ssh_target_from_command(command)
  local text = trim_surrounding_whitespace(command or "")
  if text == "" then
    return nil
  end
  return ssh_target_from_tokens(shell_split(text))
end

local function pane_domain_name(pane)
  if not pane then
    return ""
  end

  local ok, value = pcall(function()
    if pane.get_domain_name then
      return pane:get_domain_name()
    end
    return pane.domain_name
  end)
  if not ok then
    return ""
  end
  return trim_surrounding_whitespace(value or "")
end

local function pane_user_vars(pane)
  if not pane then
    return nil
  end

  local ok, vars = pcall(function()
    if pane.get_user_vars then
      return pane:get_user_vars()
    end
    return pane.user_vars
  end)
  if not ok or type(vars) ~= "table" then
    return nil
  end
  return vars
end

local function pane_foreground_process_name(pane)
  if not pane then
    return ""
  end

  local ok, value = pcall(function()
    if pane.get_foreground_process_name then
      return pane:get_foreground_process_name()
    end
    return pane.foreground_process_name
  end)
  if not ok then
    return ""
  end
  return trim_surrounding_whitespace(value or "")
end

local function pane_foreground_process_info(pane)
  if not pane then
    return nil
  end

  local ok, info = pcall(function()
    return pane:get_foreground_process_info()
  end)
  if not ok or type(info) ~= "table" then
    return nil
  end
  return info
end

local function pane_cwd_value(pane)
  if not pane then
    return nil
  end

  local ok, value = pcall(function()
    if pane.get_current_working_dir then
      return pane:get_current_working_dir()
    end
    return pane.current_working_dir
  end)
  if not ok then
    return nil
  end
  return value
end

local function resolve_remote_target_from_pane(pane)
  if not pane then
    return nil
  end

  local domain_name = pane_domain_name(pane)
  local domain_target = extract_ssh_domain_target(domain_name)
  if domain_target then
    return domain_target
  end

  local user_vars = pane_user_vars(pane)
  if type(user_vars) == "table" then
    local prog_target = ssh_target_from_command(user_vars.WEZTERM_PROG)
    if prog_target then
      return prog_target
    end
  end

  local proc_name = pane_foreground_process_name(pane)
  if basename(proc_name) == "ssh" then
    local info = pane_foreground_process_info(pane)
    if info and type(info.argv) == "table" then
      local argv_target = ssh_target_from_tokens(info.argv)
      if argv_target then
        return argv_target
      end
    end
  end

  local cwd = pane_cwd_value(pane)
  if cwd and (type(cwd) == "table" or type(cwd) == "userdata") then
    local host = trim_surrounding_whitespace(cwd.host or "")
    if host ~= "" then
      local username = trim_surrounding_whitespace(cwd.username or "")
      if username ~= "" then
        return username .. "@" .. host
      end
      return host
    end
  end

  return nil
end

local function mountpoint_is_active(path)
  local ok = select(1, run_process({
    "sh",
    "-lc",
    'mount | grep -F " on $1 (" >/dev/null',
    "kaku-remote-files",
    path,
  }))
  return ok
end

local function path_exists(path)
  if not path or path == "" then
    return false
  end
  return select(1, run_process({
    "/usr/bin/test",
    "-e",
    path,
  }))
end

local function ensure_remote_mount_root()
  if not remote_files_mount_root or remote_files_mount_root == "" then
    return false, "HOME is unavailable; cannot prepare a mount directory."
  end

  local ok, _, stderr = run_process({
    "/bin/mkdir",
    "-p",
    remote_files_mount_root,
  })
  if ok then
    return true, remote_files_mount_root
  end

  local message = trim_surrounding_whitespace(stderr)
  if message == "" then
    message = "failed to create the local mount root"
  end
  return false, message
end

local function ensure_remote_mount(sshfs_cmd, remote_target, mount_path)
  local root_ok, root_or_err = ensure_remote_mount_root()
  if not root_ok then
    return false, root_or_err
  end

  local mkdir_ok, _, mkdir_stderr = run_process({
    "/bin/mkdir",
    "-p",
    mount_path,
  })
  if not mkdir_ok then
    local message = trim_surrounding_whitespace(mkdir_stderr)
    if message == "" then
      message = "failed to create the mount path"
    end
    return false, message
  end

  if mountpoint_is_active(mount_path) then
    return true, mount_path
  end

  -- Fail fast when the SSH alias still needs an interactive password prompt.
  local ssh_ok, _, ssh_stderr = run_process({
    "/usr/bin/ssh",
    "-o",
    "BatchMode=yes",
    remote_target,
    "true",
  })
  if not ssh_ok then
    local message = trim_surrounding_whitespace(ssh_stderr)
    if message == "" then
      message = "non-interactive SSH auth failed; key or agent auth is required"
    end
    return false, message
  end

  local volume_name = "Kaku-" .. sanitize_mount_component(remote_target)
  local mount_ok, _, mount_stderr = run_process({
    sshfs_cmd,
    remote_target .. ":/",
    mount_path,
    "-o",
    "reconnect,defer_permissions,volname=" .. volume_name,
  })
  if mount_ok and mountpoint_is_active(mount_path) then
    return true, mount_path
  end

  local message = trim_surrounding_whitespace(mount_stderr)
  if message == "" then
    message = "sshfs mount failed"
  end
  return false, message
end

local function remote_open_path(mount_path, remote_cwd)
  local clean_cwd = trim_surrounding_whitespace(remote_cwd or "")
  if clean_cwd == "" or clean_cwd == "/" then
    return mount_path
  end

  local candidate
  if clean_cwd:sub(1, 1) == "/" then
    candidate = mount_path .. clean_cwd
  else
    candidate = mount_path .. "/" .. clean_cwd
  end

  if path_exists(candidate) then
    return candidate
  end
  return mount_path
end

local function open_remote_files(window, pane)
  pane = resolve_active_pane(window, pane)
  if not pane then
    show_remote_files_toast(window, "No active pane.")
    return
  end

  local remote_target = resolve_remote_target_from_pane(pane)
  if not remote_target then
    show_remote_files_toast(window, "Only SSH sessions are supported right now.")
    return
  end

  local yazi_cmd = resolve_yazi_command()
  if not yazi_cmd then
    show_remote_files_toast(window, "Yazi not found. Run kaku init.")
    return
  end

  local sshfs_cmd = resolve_sshfs_command()
  if not sshfs_cmd then
    show_remote_files_toast(window, "sshfs not found. Install macFUSE + sshfs first.", 5000)
    return
  end

  if not remote_files_mount_root or remote_files_mount_root == "" then
    show_remote_files_toast(window, "HOME is unavailable; cannot prepare a mount directory.", 5000)
    return
  end

  local mount_path = remote_files_mount_root .. "/" .. sanitize_mount_component(remote_target)
  local mount_ok, mount_or_err = ensure_remote_mount(sshfs_cmd, remote_target, mount_path)
  if not mount_ok then
    show_remote_files_toast(window, "Mount failed: " .. mount_or_err, 6000)
    return
  end

  local open_path = remote_open_path(mount_or_err, pane_cwd(pane))
  local open_ok, open_err = pcall(function()
    window:perform_action(
      wezterm.action.SpawnCommandInNewTab({
        domain = "DefaultDomain",
        cwd = open_path,
        args = { yazi_cmd, open_path },
      }),
      pane
    )
  end)
  if not open_ok then
    show_remote_files_toast(window, "Failed to open Yazi: " .. trim_surrounding_whitespace(tostring(open_err or "")), 5000)
  end
end

local function maybe_show_lazygit_hint(window, pane)
  if now_secs() < lazygit_hint_warmup_until_secs then
    return
  end

  pane = resolve_active_pane(window, pane)

  local path = pane_cwd(pane)
  if path == "" then
    return
  end

  local repo_root, has_changes = git_repo_context(path)
  if not repo_root then
    return
  end

  if pane_is_lazygit(pane) then
    mark_repo_lazygit_used(repo_root)
    return
  end

  local flags = get_lazygit_repo_flags(repo_root)
  if flags.hinted or flags.used or not has_changes then
    return
  end

  if not is_lazygit_installed() then
    return
  end

  show_lazygit_toast(window, pane, "kaku-toast-try-lazygit")
  mark_repo_lazygit_hinted(repo_root)
end

local function schedule_lazygit_hint_probe(window, pane)
  local active_pane = resolve_active_pane(window, pane)
  if not active_pane then
    return
  end

  local pane_id_ok, pane_id_value = pcall(function()
    return active_pane:pane_id()
  end)
  if not pane_id_ok or not pane_id_value then
    return
  end

  local pane_id = tostring(pane_id_value)
  local state = lazygit_hint_probe_state_by_pane[pane_id] or {}
  lazygit_hint_probe_state_by_pane[pane_id] = state

  if state.scheduled then
    return
  end

  local now = now_secs()
  if state.last_scheduled_at and (now - state.last_scheduled_at) < lazygit_hint_schedule_cooldown_secs then
    return
  end

  state.scheduled = true
  state.last_scheduled_at = now

  local delay = math.max(0, lazygit_hint_warmup_until_secs - now)
  wezterm.time.call_after(delay, function()
    state.scheduled = false
    pcall(function()
      maybe_show_lazygit_hint(window, active_pane)
    end)
  end)
end

local function launch_lazygit(window, pane)
  pane = resolve_active_pane(window, pane)
  if not pane then
    show_lazygit_toast(window, pane, "kaku-toast-lazygit-no-pane")
    return
  end

  local path = pane_cwd(pane)
  if path == "" then
    show_lazygit_toast(window, pane, "kaku-toast-lazygit-no-cwd")
    return
  end

  local repo_root = detect_git_repo_root(path)
  if not repo_root then
    show_lazygit_toast(window, pane, "kaku-toast-lazygit-not-git")
    return
  end

  local lazygit_cmd = resolve_lazygit_command()
  if not lazygit_cmd then
    show_lazygit_toast(window, pane, "kaku-toast-lazygit-missing")
    return
  end

  local ok = pcall(function()
    -- Send Ctrl+U first to clear any partially typed input at the prompt,
    -- preventing the command from being appended to existing line content.
    window:perform_action(
      wezterm.action.SendString("\x15" .. lazygit_cmd .. "\r"),
      pane
    )
  end)
  if not ok then
    show_lazygit_toast(window, pane, "kaku-toast-lazygit-dispatch-failed")
    return
  end
  mark_repo_lazygit_used(repo_root)
end

local function launch_yazi(window, pane)
  pane = resolve_active_pane(window, pane)
  if not pane then
    show_yazi_toast(window, pane, "kaku-toast-yazi-no-pane")
    return
  end

  sync_managed_yazi_theme(window)

  local yazi_cmd = resolve_yazi_command()
  if not yazi_cmd then
    set_yazi_mode_hint(pane, false)
    show_yazi_toast(window, pane, "kaku-toast-yazi-missing")
    return
  end

  set_yazi_mode_hint(pane, true)
  local dims = window:get_dimensions()
  update_window_config(window, dims.is_full_screen, pane)

  local ok = pcall(function()
    -- Prefer the shell wrapper `y` for cwd sync, and fall back to raw yazi.
    window:perform_action(
      wezterm.action.SendString("\x15y 2>/dev/null || " .. yazi_cmd .. "\r"),
      pane
    )
  end)
  if not ok then
    set_yazi_mode_hint(pane, false)
    show_yazi_toast(window, pane, "kaku-toast-yazi-dispatch-failed")
    return
  end
end

local function evict_stale_cache(live_pane_ids)
  for pane_id in pairs(active_tab_cwd_cache) do
    if not live_pane_ids[pane_id] then
      active_tab_cwd_cache[pane_id] = nil
    end
  end
end

local function tab_path_parts(tab)
  local pane = tab.active_pane
  if not pane then
    return '', ''
  end

  local source_cwd = pane.current_working_dir
  local source_path = extract_path_from_cwd(source_cwd)
  local path = source_path

  if tab.is_active then
    local pane_id = tostring(pane.pane_id)
    local now = now_secs()
    local runtime_cwd_ready = now >= runtime_cwd_warmup_until_secs
    local cached = active_tab_cwd_cache[pane_id]
    local should_refresh = (not cached)
      or path == ''
      or source_path ~= cached.source_path
      or (now - cached.updated_at) >= active_tab_cwd_refresh_interval

    if should_refresh then
      local ok, runtime_cwd = pcall(function()
        if not runtime_cwd_ready then
          return nil
        end
        return pane:get_current_working_dir()
      end)
      if ok and runtime_cwd then
        local runtime_path = extract_path_from_cwd(runtime_cwd)
        if runtime_path ~= '' then
          path = runtime_path
        end
      end

      active_tab_cwd_cache[pane_id] = {
        path = path,
        source_path = source_path,
        updated_at = now,
      }
    elseif cached and cached.path ~= '' then
      path = cached.path
    end
  elseif path == '' and now_secs() >= runtime_cwd_warmup_until_secs then
    local ok, runtime_cwd = pcall(function()
      return pane:get_current_working_dir()
    end)
    if ok and runtime_cwd then
      path = extract_path_from_cwd(runtime_cwd)
    end
  end

  if path == '' then
    return '', ''
  end

  local current = basename(path) or path
  local parent_path = path:match('(.+)/[^/]+$') or ''
  local parent = basename(parent_path) or parent_path
  return parent, current
end

-- ===== Kaku Palette =====
local KAKU_BLACK = '#15141b'
local KAKU_ANSI_BLACK = '#110f18'
local KAKU_WHITE = '#edecee'
local KAKU_GRAY = '#6d6d6d'
local KAKU_PURPLE = '#a277ff'
-- Use rgba() here because config::RgbaColor does not accept #RRGGBBAA.
local KAKU_PURPLE_FADING = 'rgba(61,55,94,0.5)'
local KAKU_SURFACE = '#1f1d28'
local KAKU_SURFACE_ACTIVE = '#29263c'
local KAKU_GREEN = '#61ffca'
local KAKU_ORANGE = '#ffca85'
local KAKU_PINK = '#f694ff'
local KAKU_BLUE = '#5fa8ff'
local KAKU_BRIGHT_BLUE = '#8cc2ff'
local KAKU_RED = '#ff6767'

-- Track bell events per pane for tab notification indicator.
-- Unlike has_unseen_output (which fires on any output, making the indicator
-- permanently lit for TUI apps like Claude Code), bell events only fire when
-- a program explicitly sends BEL (\a), making them suitable as completion signals.
local _bell_panes = {}

wezterm.on('bell', function(window, pane)
  _bell_panes[tostring(pane:pane_id())] = true
end)

local function evict_stale_bell_panes(live_pane_ids)
  for pane_id in pairs(_bell_panes) do
    if not live_pane_ids[pane_id] then
      _bell_panes[pane_id] = nil
    end
  end
end

local function tab_pane_keys(tab)
  local keys = {}
  if not tab then
    return keys
  end

  if type(tab.panes) == 'table' then
    for _, pane in ipairs(tab.panes) do
      if pane and pane.pane_id then
        keys[#keys + 1] = tostring(pane.pane_id)
      end
    end
  end

  if #keys == 0 and tab.active_pane and tab.active_pane.pane_id then
    keys[1] = tostring(tab.active_pane.pane_id)
  end

  return keys
end

local function tab_has_bell(tab)
  for _, pane_key in ipairs(tab_pane_keys(tab)) do
    if _bell_panes[pane_key] then
      return true
    end
  end
  return false
end

local function clear_tab_bells(tab)
  for _, pane_key in ipairs(tab_pane_keys(tab)) do
    _bell_panes[pane_key] = nil
  end
end

wezterm.on('format-tab-title', function(tab, tabs, _, effective_config, hover, max_width)
  -- Evict stale cache only on the first tab to avoid O(n²) across the render cycle
  if tab.tab_index == 0 then
    local live_pane_ids = {}
    for _, t in ipairs(tabs) do
      for _, pane_key in ipairs(tab_pane_keys(t)) do
        live_pane_ids[pane_key] = true
      end
    end
    evict_stale_cache(live_pane_ids)
    evict_stale_bell_panes(live_pane_ids)
  end

  -- Use user-set tab title if available
  local text = tab.tab_title or ''
  if text == '' then
    local parent, current = tab_path_parts(tab)
    local basename_only = effective_config and effective_config.tab_title_show_basename_only
    text = current
    if not basename_only and parent ~= '' and current ~= '' then
      text = parent .. '/' .. current
    end
  end

  -- Guard active_pane nil before accessing .title / .is_zoomed
  local active_pane = tab.active_pane
  if text == '' and active_pane then
    text = active_pane.title
  end
  if active_pane and active_pane.is_zoomed then
    text = text .. ' [Z]'
  end
  text = wezterm.truncate_right(text, math.max(8, max_width - 2))

  local intensity = tab.is_active and 'Bold' or 'Normal'
  -- resolved_palette.tab_bar and its sub-fields are all optional; guard each level
  local tab_bar_colors = effective_config.resolved_palette.tab_bar
  local fg
  if tab_bar_colors then
    local entry
    if tab.is_active then
      entry = tab_bar_colors.active_tab
    elseif hover then
      entry = tab_bar_colors.inactive_tab_hover or tab_bar_colors.inactive_tab
    else
      entry = tab_bar_colors.inactive_tab
    end
    fg = entry and entry.fg_color
  end
  -- fallback defaults when palette entry or sub-field is absent
  if not fg then
    fg = tab.is_active and KAKU_WHITE or (hover and KAKU_WHITE or KAKU_GRAY)
  end

  local has_bell = tab_has_bell(tab)
  if has_bell and tab.is_active then
    clear_tab_bells(tab)
    has_bell = false
  end

  -- Bell-based suffix indicator: show a small dot after the title when a BEL
  -- was received, and honor the standard bell_tab_indicator toggle.
  if has_bell and effective_config.bell_tab_indicator ~= false then
    return {
      { Attribute = { Intensity = intensity } },
      { Foreground = { Color = fg } },
      { Text = ' ' .. text .. ' ' },
      { Foreground = { Color = KAKU_PURPLE } },
      { Text = '• ' },
    }
  end

  return {
    { Attribute = { Intensity = intensity } },
    { Foreground = { Color = fg } },
    { Text = ' ' .. text .. ' ' },
  }
end)

wezterm.on('window-resized', function(window, _)
  local dims = window:get_dimensions()
  local active_pane = nil
  local ok_tab, tab = pcall(function()
    return window:active_tab()
  end)
  if ok_tab and tab then
    local ok_pane, pane = pcall(function()
      return tab:active_pane()
    end)
    if ok_pane then
      active_pane = pane
    end
  end
  update_window_config(window, dims.is_full_screen, active_pane)
end)

wezterm.on('kaku-launch-lazygit', function(window, pane)
  launch_lazygit(window, pane)
end)

wezterm.on('kaku-launch-yazi', function(window, pane)
  launch_yazi(window, pane)
end)

wezterm.on('kaku-open-remote-files', function(window, pane)
  open_remote_files(window, pane)
end)

wezterm.on('kaku-ai-apply-last-fix', function(window, pane)
  pane = resolve_active_pane(window, pane)
  if not pane then
    return
  end

  local pane_id_ok, pane_id_value = pcall(function()
    return pane:pane_id()
  end)
  if not pane_id_ok or not pane_id_value then
    inject_ai_status(pane, "Unable to access the active pane.")
    return
  end

  local pane_state = ai_fix_state_by_pane[tostring(pane_id_value)]
  local suggestion = pane_state and pane_state.suggestion or nil
  local command = suggestion and sanitize_suggested_command(suggestion.command or "") or ""
  if command == "" then
    inject_ai_status(pane, "No quick fix command is available.")
    return
  end

  local send_text = command .. "\n"
  local toast_message = "Applied suggested command."
  if is_dangerous_command(command) then
    send_text = command
    toast_message = "Suggested command pasted only. Please review before running."
  end

  local sent_ok = pcall(function()
    pane:send_text("\x15" .. send_text)
  end)
  if not sent_ok then
    inject_ai_status(pane, "Unable to send the suggested command.")
    return
  end

  if is_dangerous_command(command) then
    inject_ai_status(pane, toast_message)
  end
end)

wezterm.on('user-var-changed', function(window, pane, name, value)
  ai_debug_log("user-var-changed name=" .. tostring(name) .. " value=" .. tostring(value))
  if name == "kaku_last_cmd" then
    if not pane then
      return
    end

    local pane_id_ok, pane_id_value = pcall(function()
      return pane:pane_id()
    end)
    if not pane_id_ok or not pane_id_value then
      return
    end

    local pane_id = tostring(pane_id_value)
    local pane_state = ai_fix_state_by_pane[pane_id]
    if not pane_state or not pane_state.inflight then
      return
    end

    pane_state.inflight = false
    pane_state.pending_job_id = nil
    clear_ai_fix_suggestion_state(pane_state)
    ai_debug_log("user-var-changed cancelled inflight ai fix pane_id=" .. pane_id)
    return
  end

  if name == "kaku_user_typing" then
    if not pane then
      return
    end

    local pane_id_ok, pane_id_value = pcall(function()
      return pane:pane_id()
    end)
    if not pane_id_ok or not pane_id_value then
      return
    end

    local pane_id = tostring(pane_id_value)
    local pane_state = ai_fix_state_by_pane[pane_id]
    if not pane_state or not pane_state.inflight then
      return
    end

    pane_state.inflight = false
    pane_state.pending_job_id = nil
    clear_ai_fix_suggestion_state(pane_state)
    ai_debug_log("user-var-changed user typing cancelled ai fix pane_id=" .. pane_id)
    return
  end

  if name ~= "kaku_last_exit_code" then
    return
  end
  if not pane then
    ai_debug_log("user-var-changed ignored no pane")
    return
  end

  local exit_code = tonumber(value)
  if not exit_code then
    ai_debug_log("user-var-changed ignored invalid exit_code")
    return
  end
  if exit_code == 0 then
    ai_debug_log("user-var-changed ignored exit_code=0")
    return
  end
  if exit_code == 130 then
    ai_debug_log("user-var-changed ignored exit_code=130")
    return
  end
  if exit_code == 141 then
    ai_debug_log("user-var-changed ignored exit_code=141")
    return
  end

  local pane_id_ok, pane_id_value = pcall(function()
    return pane:pane_id()
  end)
  if not pane_id_ok or not pane_id_value then
    ai_debug_log("user-var-changed ignored pane_id unavailable")
    return
  end
  local pane_id = tostring(pane_id_value)
  local pane_state = ai_fix_state_by_pane[pane_id] or {}
  ai_fix_state_by_pane[pane_id] = pane_state

  if pane_state.inflight then
    ai_debug_log("user-var-changed skipped inflight pane_id=" .. pane_id)
    return
  end

  local vars_ok, vars = pcall(function()
    return pane:get_user_vars()
  end)
  if not vars_ok or type(vars) ~= "table" then
    ai_debug_log("user-var-changed ignored no user vars")
    return
  end

  local failed_command = trim_surrounding_whitespace(vars.kaku_last_cmd or "")
  if failed_command == "" then
    ai_debug_log("user-var-changed ignored missing kaku_last_cmd")
    return
  end
  if should_skip_ai_fix_for_failed_command(failed_command, exit_code) then
    ai_debug_log("user-var-changed skipped by non-error command policy")
    return
  end
  ai_debug_log("user-var-changed failed_command=" .. failed_command .. " exit=" .. tostring(exit_code))

  local signature = failed_command .. "\0" .. tostring(exit_code)
  local now = now_secs()
  if pane_state.last_signature == signature and (now - (pane_state.last_seen_at or 0)) <= 1 then
    ai_debug_log("user-var-changed skipped duplicate signature")
    return
  end
  pane_state.last_signature = signature
  pane_state.last_seen_at = now

  clear_ai_fix_suggestion_state(pane_state)
  pane_state.failed_command = failed_command
  pane_state.exit_code = exit_code

  refresh_ai_fix_settings()
  if not ai_fix_enabled then
    ai_debug_log("user-var-changed ai_fix disabled after refresh")
    return
  end

  if not ai_fix_api_key or ai_fix_api_key == "" then
    ai_debug_log("user-var-changed missing api key after refresh")
    return
  end

  -- Skip AI fix if foreground process is not a shell.
  -- This prevents injecting input into interactive programs like claude, vim, ssh.
  if not is_shell_foreground(pane) then
    ai_debug_log("user-var-changed skipped non-shell foreground process")
    return
  end

  pane_state.inflight = true
  pane_state.pending_job_id = nil
  show_ai_loading_toast(window, pane)

  local cwd = pane_cwd(pane)
  local git_branch = detect_git_branch(cwd)
  local ok, err = request_ai_fix_async(window, pane, pane_id, failed_command, exit_code, cwd, git_branch)
  if not ok then
    pane_state.inflight = false
    pane_state.pending_job_id = nil
    clear_ai_fix_suggestion_state(pane_state)
    ai_debug_log("user-var-changed ai request start failed err=" .. tostring(err))
    inject_ai_status_and_finalize(pane, "Could not analyze this error right now.")
  end
end)

wezterm.on('update-right-status', function(window, pane)
  pane = resolve_active_pane(window, pane)
  if remember_last_cwd then
    local ok, cwd = pcall(function() return pane:get_current_working_dir() end)
    if ok and cwd then
      -- Url userdata: .host is nil for local file:/// URLs, hostname string for SSH.
      local is_local = (cwd.host == nil or cwd.host == '')
      if is_local then
        save_last_cwd(extract_path_from_cwd(cwd))
      end
    end
  end
  schedule_lazygit_hint_probe(window, pane)

  local dims = window:get_dimensions()
  update_window_config(window, dims.is_full_screen, pane)
  if not dims.is_full_screen then
    window:set_right_status('')
    return
  end

  local clock_icon = wezterm.nerdfonts.md_clock_time_four_outline
    or wezterm.nerdfonts.md_clock_outline
    or ''
  local text = wezterm.strftime('%H:%M')
  if clock_icon ~= '' then
    window:set_right_status(wezterm.format({
      { Foreground = { Color = KAKU_GRAY } },
      { Text = ' ' .. clock_icon .. ' ' .. text .. ' ' },
    }))
    return
  end
  window:set_right_status(wezterm.format({
    { Foreground = { Color = KAKU_GRAY } },
    { Text = ' ' .. text .. ' ' },
  }))
end)

-- ===== Font =====
-- Use slightly heavier font weight for light theme to improve readability.
-- Light theme: Medium base, SemiBold for bold.
-- Dark theme: Regular base, Medium for bold.
local function build_font_config(is_light)
  local base_weight = is_light and 'Medium' or 'Regular'
  local bold_weight = is_light and 'SemiBold' or 'Medium'

  local font = wezterm.font_with_fallback({
    { family = 'JetBrains Mono', weight = base_weight },
    { family = 'PingFang SC', weight = base_weight },
    'Apple Color Emoji',
  })

  local font_rules = {
    -- Prevent thin weight: use base weight instead of Light for Half intensity
    {
      intensity = 'Half',
      font = wezterm.font_with_fallback({
        { family = 'JetBrains Mono', weight = base_weight },
        { family = 'PingFang SC', weight = base_weight },
      }),
    },
    -- Normal italic: disable real italics (keep upright)
    {
      intensity = 'Normal',
      italic = true,
      font = wezterm.font_with_fallback({
        { family = 'JetBrains Mono', weight = base_weight, italic = false },
        { family = 'PingFang SC', weight = base_weight },
      }),
    },
    -- Bold: use heavier weight
    {
      intensity = 'Bold',
      font = wezterm.font_with_fallback({
        { family = 'JetBrains Mono', weight = bold_weight },
        { family = 'PingFang SC', weight = bold_weight },
      }),
    },
  }

  return font, font_rules
end

-- Check user config to determine initial theme for font weight
local function is_user_light_theme()
  local user_config_path = kaku_user_config_path()
  if not user_config_path then
    return false
  end
  local file = io.open(user_config_path, 'r')
  if not file then
    return false
  end
  for line in file:lines() do
    local trimmed = line:match('^%s*(.-)%s*$')
    if trimmed and not trimmed:match('^%-%-') then
      if trimmed:match("^config%.color_scheme%s*=%s*['\"]Kaku Light['\"]") then
        file:close()
        return true
      end
      if trimmed:match("^config%.color_scheme%s*=%s*['\"]Kaku Dark['\"]") then
        file:close()
        return false
      end
      if trimmed:match('^config%.color_scheme%s*=') and trimmed:match('get_appearance') then
        file:close()
        return resolve_appearance_color_scheme() == 'Kaku Light'
      end
    end
  end
  file:close()
  return false
end

-- Only seed the managed default font stack when the user hasn't overridden it.
-- The bundled font_rules are tightly coupled to the bundled JetBrains Mono stack,
-- so they must not remain active when the user selects a custom primary font.
do
  local font, font_rules = build_font_config(is_user_light_theme())
  if not user_has_custom_font then
    config.font = font
  end
  if not user_has_custom_font and not user_has_custom_font_rules then
    config.font_rules = font_rules
  end
end

-- Track last font theme per window to avoid redundant overrides
local window_font_theme = setmetatable({}, { __mode = 'k' })
local window_has_managed_font_override = setmetatable({}, { __mode = 'k' })

-- Dynamically switch font weight when theme changes
wezterm.on('window-config-reloaded', function(window, pane)
  local overrides = window:get_config_overrides() or {}
  local scheme = resolve_kaku_color_scheme(overrides.color_scheme or config.color_scheme)
  local is_light = scheme == 'Kaku Light'

  if user_has_custom_font or user_has_custom_font_rules then
    window_font_theme[window] = nil
    if window_has_managed_font_override[window]
      and (overrides.font ~= nil or overrides.font_rules ~= nil) then
      overrides.font = nil
      overrides.font_rules = nil
      window:set_config_overrides(overrides)
    end
    window_has_managed_font_override[window] = nil
  elseif window_font_theme[window] ~= is_light then
    window_font_theme[window] = is_light

    local font, font_rules = build_font_config(is_light)
    overrides.font = font
    overrides.font_rules = font_rules
    window_has_managed_font_override[window] = true
    window:set_config_overrides(overrides)
  end

  local dims = window:get_dimensions()
  update_window_config(window, dims.is_full_screen, pane)
end)

config.bold_brightens_ansi_colors = false

-- Auto-adjust font size using main-screen pixel size.
-- low-resolution screens use 15px.
-- high-resolution screens use 17px.
local function get_font_size()
  if low_resolution_screen then
    return 15.0
  end

  local success, screens = pcall(function()
    return wezterm.gui.screens()
  end)
  if success and screens and screens.main then
    local main = screens.main
    -- Fallback when pixel dimensions are unavailable.
    local dpi = tonumber(main.effective_dpi or 72) or 72
    if dpi < 110 then
      return 15.0
    end
  end
  return 17.0
end

config.font_size = get_font_size()
config.line_height = 1.28
config.cell_width = 1.0
config.harfbuzz_features = { 'calt=0', 'clig=0', 'liga=0' }
config.use_cap_height_to_scale_fallback_fonts = false

config.custom_block_glyphs = true
config.unicode_version = 14

-- Do NOT set config.term = 'kaku' here.
-- Remote servers lack the 'kaku' terminfo entry, causing SSH issues like
-- broken backspace/delete keys. Let the default 'xterm-256color' apply.
-- See: https://github.com/tw93/Kaku/issues/130

-- ===== Cursor =====
config.default_cursor_style = 'BlinkingBar'
config.cursor_thickness = '2px'
config.cursor_blink_rate = 500
-- Sharp on/off blink without fade animation (like a standard terminal).
config.cursor_blink_ease_in = 'Constant'
config.cursor_blink_ease_out = 'Constant'

-- ===== Scrollback =====
config.scrollback_lines = 10000

-- ===== Text Selection =====
config.selection_word_boundary = ' \t\n{}[]()"\'-'  -- Smart selection boundaries

-- ===== Window Layout =====
config.use_resize_increments = true

config.initial_cols = 110
config.initial_rows = 22
-- Keep native macOS window shadow by default.
config.window_decorations = "INTEGRATED_BUTTONS|RESIZE"
-- Window frame colors will be set after color_scheme is determined

config.window_background_opacity = 1.0
config.text_background_opacity = 1.0

-- ===== Close Protection =====
config.window_close_confirmation = 'NeverPrompt'
-- Off by default. When enabled, Kaku always asks before closing tabs/panes.
config.tab_close_confirmation = false
config.pane_close_confirmation = false

-- ===== Tab Bar =====
config.enable_tab_bar = true
config.tab_bar_at_bottom = true
config.use_fancy_tab_bar = false
config.tab_max_width = 32
config.hide_tab_bar_if_only_one_tab = true
config.show_tab_index_in_tab_bar = true
config.show_new_tab_button_in_tab_bar = false

-- Compute padding after tab-bar placement is finalized so startup layout
-- matches the runtime override path.
config.window_padding = get_default_padding()

-- ===== Color Scheme =====
local kaku_theme = {
  -- Background
  foreground = KAKU_WHITE,
  background = KAKU_BLACK,

  -- Cursor
  cursor_bg = KAKU_PURPLE,
  cursor_fg = KAKU_BLACK,
  cursor_border = KAKU_PURPLE,

  -- Selection
  selection_bg = KAKU_PURPLE_FADING,
  selection_fg = 'none',

  -- Normal colors (ANSI 0-7)
  ansi = {
    KAKU_ANSI_BLACK, -- black
    KAKU_RED,     -- red
    KAKU_GREEN,   -- green
    KAKU_ORANGE,  -- yellow
    KAKU_BLUE,    -- blue
    KAKU_PURPLE,  -- magenta
    KAKU_GREEN,   -- cyan
    KAKU_WHITE,   -- white
  },

  -- Bright colors (ANSI 8-15)
  brights = {
    KAKU_GRAY,    -- bright black
    KAKU_RED,     -- bright red
    KAKU_GREEN,   -- bright green
    KAKU_ORANGE,  -- bright yellow
    KAKU_BRIGHT_BLUE, -- bright blue
    KAKU_PURPLE,  -- bright magenta
    KAKU_GREEN,   -- bright cyan
    KAKU_WHITE,   -- bright white
  },

  split = KAKU_SURFACE_ACTIVE,

  -- Tab bar colors
  tab_bar = {
    background = KAKU_BLACK,
    inactive_tab_edge = KAKU_BLACK,

    active_tab = {
      bg_color = KAKU_SURFACE_ACTIVE,
      fg_color = KAKU_WHITE,
      intensity = 'Bold',
      underline = 'None',
      italic = false,
      strikethrough = false,
    },

    inactive_tab = {
      bg_color = KAKU_BLACK,
      fg_color = KAKU_GRAY,
      intensity = 'Normal',
    },

    inactive_tab_hover = {
      bg_color = KAKU_SURFACE,
      fg_color = KAKU_WHITE,
      italic = false,
    },

    new_tab = {
      bg_color = KAKU_BLACK,
      fg_color = KAKU_GRAY,
    },

    new_tab_hover = {
      bg_color = KAKU_SURFACE,
      fg_color = KAKU_WHITE,
    },
  },

  -- Override Claude Code quote background for better contrast
  color_overrides = {
    ['#6d6d6d'] = '#3A3942',  -- ANSI 8 (bright black)
    ['#6E6E6E'] = '#3A3942',  -- Claude Code true color
    ['#8EC3FF'] = '#3A3942',  -- Claude Code blue header background
  },
}

-- ===== Kaku Light Theme =====
local kaku_light = {
  foreground = '#100F0F',
  background = '#FFFCF0',

  cursor_bg = '#343331',
  cursor_fg = '#FFFCF0',
  cursor_border = '#343331',

  selection_bg = '#E8E6DB',
  selection_fg = '#100F0F',

  ansi = {
    '#100F0F', -- black
    '#AF3029', -- red-600
    '#66800B', -- green-600
    '#AD8301', -- yellow-600
    '#205EA6', -- blue-600
    '#A02F6F', -- magenta-600
    '#24837B', -- cyan-600
    '#575653', -- base-700
  },

  brights = {
    '#6F6E69', -- base-600 (comments)
    '#C03E35', -- red-500
    '#768D21', -- green-500
    '#BE9207', -- yellow-500
    '#3171B2', -- blue-500
    '#B74583', -- magenta-500
    '#2F968D', -- cyan-500
    '#403E3C', -- base-800
  },

  scrollbar_thumb = '#C9C2B1',
  split = '#B8B7AD',

  tab_bar = {
    background = '#FFFCF0',
    inactive_tab_edge = '#FFFCF0',

    active_tab = {
      bg_color = '#E8E6DB',
      fg_color = '#100F0F',
      intensity = 'Bold',
      underline = 'None',
      italic = false,
      strikethrough = false,
    },

    inactive_tab = {
      bg_color = '#FFFCF0',
      fg_color = '#4A4946',
      intensity = 'Normal',
    },

    inactive_tab_hover = {
      bg_color = '#E8E6DB',
      fg_color = '#100F0F',
      italic = false,
    },

    new_tab = {
      bg_color = '#FFFCF0',
      fg_color = '#4A4946',
    },

    new_tab_hover = {
      bg_color = '#E8E6DB',
      fg_color = '#100F0F',
    },
  },

  -- Override Claude Code quote background for better contrast
  color_overrides = {
    ['#575653'] = '#F2F0EB',  -- ANSI 7 (white)
    ['#585754'] = '#F2F0EB',  -- Claude Code true color
    ['#225FA6'] = '#F2F0EB',  -- Claude Code blue header background
  },
}

config.color_schemes = config.color_schemes or {}
config.color_schemes['Kaku Dark'] = kaku_theme
config.color_schemes['Kaku Light'] = kaku_light
-- Legacy alias for compatibility
config.color_schemes['Kaku Theme'] = kaku_theme
config.color_scheme = resolve_kaku_color_scheme(config.color_scheme)

-- ===== Window Frame (theme-aware) =====
local function get_window_frame_colors()
  local scheme = resolve_kaku_color_scheme(config.color_scheme)
  if scheme == 'Kaku Light' then
    return '#FFFCF0', '#FFFCF0'
  else
    return KAKU_BLACK, KAKU_BLACK
  end
end

local active_titlebar_bg, inactive_titlebar_bg = get_window_frame_colors()
config.window_frame = {
  font = wezterm.font({ family = 'JetBrains Mono', weight = 'Regular' }),
  font_size = 13.0,
  active_titlebar_bg = active_titlebar_bg,
  inactive_titlebar_bg = inactive_titlebar_bg,
}

-- ===== Shell =====
local user_shell = os.getenv('SHELL')
if user_shell and #user_shell > 0 then
  config.default_prog = { user_shell, '-l' }
else
  config.default_prog = { '/bin/zsh', '-l' }
end

-- ===== macOS Specific =====
-- Keep Left Option as Meta so Alt-based Vim/Neovim keybindings work reliably.
config.send_composed_key_when_left_alt_is_pressed = false
-- Keep Right Option available for composing locale/symbol characters.
config.send_composed_key_when_right_alt_is_pressed = true
config.native_macos_fullscreen_mode = true
config.quit_when_all_windows_are_closed = false

-- ===== Key Bindings =====
config.keys = {
  -- Window & App
  -- Cmd+K: clear screen + scrollback
  {
    key = 'k',
    mods = 'CMD',
    action = wezterm.action.Multiple({
      wezterm.action.SendKey({ key = 'l', mods = 'CTRL' }),
      wezterm.action.ClearScrollback('ScrollbackAndViewport'),
    }),
  },

  -- Compatibility: keep Cmd+R for existing muscle memory
  {
    key = 'r',
    mods = 'CMD',
    action = wezterm.action.Multiple({
      wezterm.action.SendKey({ key = 'l', mods = 'CTRL' }),
      wezterm.action.ClearScrollback('ScrollbackAndViewport'),
    }),
  },

  -- Cmd+Q: quit
  {
    key = 'q',
    mods = 'CMD',
    action = wezterm.action.QuitApplication,
  },

  -- Cmd+N: new window
  {
    key = 'n',
    mods = 'CMD',
    action = wezterm.action.SpawnWindow,
  },

  -- Close Behavior
  -- Cmd+W: close pane > close tab > hide app
  {
    key = 'w',
    mods = 'CMD',
    action = wezterm.action_callback(function(win, pane)
      local mux_win = win:mux_window()
      local tabs = mux_win and mux_win:tabs() or {}
      local current_tab = pane:tab()
      local panes = current_tab and current_tab:panes() or {}
      if #panes > 1 then
        win:perform_action(wezterm.action.CloseCurrentPane { confirm = config.pane_close_confirmation }, pane)
      else
        local should_close_tab = (#tabs > 1) or (#wezterm.mux.all_windows() > 1)
        if should_close_tab then
          win:perform_action(wezterm.action.CloseCurrentTab { confirm = config.tab_close_confirmation }, pane)
          return
        end
        win:perform_action(wezterm.action.HideApplication, pane)
      end
    end),
  },

  -- Cmd+Shift+W: close current tab
  {
    key = 'w',
    mods = 'CMD|SHIFT',
    action = wezterm.action.CloseCurrentTab({ confirm = config.tab_close_confirmation }),
  },

  -- Tabs & Panes
  -- Cmd+T: new tab
  {
    key = 't',
    mods = 'CMD',
    action = wezterm.action.SpawnTab('CurrentPaneDomain'),
  },

  -- Cmd+Shift+A: open Kaku AI settings in current pane
  {
    key = 'A',
    mods = 'CMD|SHIFT',
    action = wezterm.action.EmitEvent('run-kaku-ai-config'),
  },

  -- Cmd+Shift+E: apply latest Kaku Assistant suggestion for the active pane
  {
    key = 'E',
    mods = 'CMD|SHIFT',
    action = wezterm.action.EmitEvent('kaku-ai-apply-last-fix'),
  },

  -- Cmd+Shift+G: launch lazygit in current pane
  {
    key = 'G',
    mods = 'CMD|SHIFT',
    action = wezterm.action.EmitEvent('kaku-launch-lazygit'),
  },

  -- Cmd+Shift+Y: launch yazi in current pane
  {
    key = 'Y',
    mods = 'CMD|SHIFT',
    action = wezterm.action.EmitEvent('kaku-launch-yazi'),
  },

  -- Cmd+Shift+R: open the current SSH domain in a local yazi tab via sshfs
  {
    key = 'R',
    mods = 'CMD|SHIFT',
    action = wezterm.action.EmitEvent('kaku-open-remote-files'),
  },

  -- Window Controls
  -- Cmd+Ctrl+F: toggle fullscreen
  {
    key = 'f',
    mods = 'CMD|CTRL',
    action = wezterm.action.ToggleFullScreen,
  },

  -- Cmd+M: minimize window
  {
    key = 'm',
    mods = 'CMD',
    action = wezterm.action.Hide,
  },

  -- Cmd+H: hide application
  {
    key = 'h',
    mods = 'CMD',
    action = wezterm.action.HideApplication,
  },

  -- Font Size
  -- Cmd+Equal/Minus/0: adjust font size
  {
    key = '=',
    mods = 'CMD',
    action = wezterm.action.IncreaseFontSize,
  },
  {
    key = '-',
    mods = 'CMD',
    action = wezterm.action.DecreaseFontSize,
  },
  {
    key = '0',
    mods = 'CMD',
    action = wezterm.action.ResetFontSize,
  },

  -- Shell Editing
  -- Alt+Left / Alt+Right: word jump
  {
    key = 'LeftArrow',
    mods = 'OPT',
    action = wezterm.action.SendKey({ key = 'b', mods = 'ALT' }),
  },
  {
    key = 'RightArrow',
    mods = 'OPT',
    action = wezterm.action.SendKey({ key = 'f', mods = 'ALT' }),
  },

  -- Cmd+Left / Cmd+Right: line start/end
  {
    key = 'LeftArrow',
    mods = 'CMD',
    action = wezterm.action.SendKey({ key = 'a', mods = 'CTRL' }),
  },
  {
    key = 'RightArrow',
    mods = 'CMD',
    action = wezterm.action.SendKey({ key = 'e', mods = 'CTRL' }),
  },

  -- Cmd+Backspace: delete to line start
  {
    key = 'Backspace',
    mods = 'CMD',
    action = wezterm.action.SendKey({ key = 'u', mods = 'CTRL' }),
  },

  -- Alt+Backspace: delete word
  {
    key = 'Backspace',
    mods = 'OPT',
    action = wezterm.action.SendKey({ key = 'w', mods = 'CTRL' }),
  },

  -- Layout
  -- Cmd+D: vertical split
  {
    key = 'd',
    mods = 'CMD',
    action = wezterm.action.SplitHorizontal({ domain = 'CurrentPaneDomain' }),
  },

  -- Cmd+Shift+D: horizontal split
  {
    key = 'D',
    mods = 'CMD|SHIFT',
    action = wezterm.action.SplitVertical({ domain = 'CurrentPaneDomain' }),
  },

  -- Cmd+Shift+[ / ]: prev/next tab
  {
    key = '[',
    mods = 'CMD|SHIFT',
    action = wezterm.action.ActivateTabRelative(-1),
  },
  {
    key = ']',
    mods = 'CMD|SHIFT',
    action = wezterm.action.ActivateTabRelative(1),
  },

  -- Pane Navigation
  -- Cmd+Option+Arrow: navigate between splits
  {
    key = 'LeftArrow',
    mods = 'CMD|OPT',
    action = wezterm.action.ActivatePaneDirection('Left'),
  },
  {
    key = 'RightArrow',
    mods = 'CMD|OPT',
    action = wezterm.action.ActivatePaneDirection('Right'),
  },
  {
    key = 'UpArrow',
    mods = 'CMD|OPT',
    action = wezterm.action.ActivatePaneDirection('Up'),
  },
  {
    key = 'DownArrow',
    mods = 'CMD|OPT',
    action = wezterm.action.ActivatePaneDirection('Down'),
  },

  -- Cmd+1~9: switch tab
  {
    key = '1',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(0),
  },
  {
    key = '2',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(1),
  },
  {
    key = '3',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(2),
  },
  {
    key = '4',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(3),
  },
  {
    key = '5',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(4),
  },
  {
    key = '6',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(5),
  },
  {
    key = '7',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(6),
  },
  {
    key = '8',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(7),
  },
  {
    key = '9',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(8),
  },

  -- Command Input
  -- Cmd+Enter / Shift+Enter: newline without execute
  {
    key = 'Enter',
    mods = 'CMD',
    action = wezterm.action.SendString('\n'),
  },
  {
    key = 'Enter',
    mods = 'SHIFT',
    action = wezterm.action.SendString('\n'),
  },

  -- Cmd+Shift+Enter: Toggle Pane Zoom (Maximize active pane)
  {
    key = 'Enter',
    mods = 'CMD|SHIFT',
    action = wezterm.action.TogglePaneZoomState,
  },

  -- Cmd+Shift+S: Toggle split direction (horizontal <-> vertical)
  {
    key = 'S',
    mods = 'CMD|SHIFT',
    action = wezterm.action.TogglePaneSplitDirection,
  },

  -- Cmd+Ctrl+Arrows: Resize panes
  {
    key = 'LeftArrow',
    mods = 'CMD|CTRL',
    action = wezterm.action.AdjustPaneSize { 'Left', 5 },
  },
  {
    key = 'RightArrow',
    mods = 'CMD|CTRL',
    action = wezterm.action.AdjustPaneSize { 'Right', 5 },
  },
  {
    key = 'UpArrow',
    mods = 'CMD|CTRL',
    action = wezterm.action.AdjustPaneSize { 'Up', 5 },
  },
  {
    key = 'DownArrow',
    mods = 'CMD|CTRL',
    action = wezterm.action.AdjustPaneSize { 'Down', 5 },
  },


}

-- ===== Mouse Bindings =====
-- Copy on select (equivalent to Kitty's copy_on_select)
-- config.copy_on_select = false -- uncomment to disable copy and toast on selection
config.mouse_bindings = {
  {
    event = { Up = { streak = 1, button = 'Left' } },
    mods = 'NONE',
    action = wezterm.action.CompleteSelection('ClipboardAndPrimarySelection'),
  },
  {
    event = { Up = { streak = 1, button = 'Left' } },
    mods = 'CMD',
    action = wezterm.action.OpenLinkAtMouseCursor,
  },
}

-- ===== Rendering & Performance =====
config.enable_scroll_bar = false
config.front_end = 'WebGpu'
config.webgpu_power_preference = 'LowPower'
config.animation_fps = 60
config.max_fps = 60
config.status_update_interval = 1000

-- ===== Pane Layout & Focus =====
-- Split pane gap: gutter = 1 + 2*gap cells, giving ~40px padding on each side
config.split_pane_gap = 2

-- Inactive panes: No dimming (consistent background)
config.inactive_pane_hsb = {
  saturation = 1.0,
  brightness = 1.0,
}

-- Prevent accidental clicks when focusing panes
config.swallow_mouse_click_on_pane_focus = true
config.swallow_mouse_click_on_window_focus = true

-- ===== First Run Experience & Config Version Check =====
wezterm.on('gui-startup', function(cmd)
  lazygit_hint_warmup_until_secs = now_secs() + lazygit_hint_startup_grace_secs
  runtime_cwd_warmup_until_secs = now_secs() + runtime_cwd_startup_grace_secs

  local home = os.getenv("HOME")
  local function read_current_config_version()
    local candidates = {
      wezterm.executable_dir:gsub("MacOS/?$", "Resources") .. "/config_version.txt",
      wezterm.executable_dir .. "/../../assets/shell-integration/config_version.txt",
    }

    for _, path in ipairs(candidates) do
      local version_file = io.open(path, "r")
      if version_file then
        local raw = version_file:read("*all")
        version_file:close()

        if raw then
          local version = tonumber(raw:match("%d+"))
          if version then
            return version
          end
        end
      end
    end

    wezterm.log_error("Failed to resolve bundled config version; falling back to v15")
    return 15
  end

  local current_version = read_current_config_version()

  local state_file = home .. "/.config/kaku/state.json"
  local is_first_run = false
  local needs_update = false

  local function ensure_state_dir()
    os.execute("mkdir -p " .. home .. "/.config/kaku")
  end

  local function write_state(version, geometry)
    ensure_state_dir()
    local state = {
      config_version = version,
    }
    if geometry and geometry.width and geometry.height then
      state.window_geometry = {
        width = geometry.width,
        height = geometry.height,
      }
    end

    local encoded = nil
    local ok, value = pcall(wezterm.json_encode, state)
    if ok and type(value) == "string" and value ~= "" then
      encoded = value
    end

    local wf = io.open(state_file, "w")
    if wf then
      if encoded then
        wf:write(encoded .. "\n")
      else
        -- Manual JSON fallback when json_encode is unavailable.
        -- Include geometry if present so state is not lost.
        if geometry and geometry.width and geometry.height then
          wf:write(string.format(
            '{\n  "config_version": %d,\n  "window_geometry": {\n    "width": %d,\n    "height": %d\n  }\n}\n',
            version, geometry.width, geometry.height))
        else
          wf:write(string.format('{\n  "config_version": %d\n}\n', version))
        end
      end
      wf:close()
    end
  end

  local user_version = nil
  local state_file_exists = false
  local sf = io.open(state_file, "r")
  if sf then
    state_file_exists = true
    local raw_state = sf:read("*all")
    sf:close()
    if raw_state and raw_state ~= "" then
      local ok, state = pcall(wezterm.json_parse, raw_state)
      if ok and type(state) == "table" then
        user_version = tonumber(state.config_version)
      end
    end
  end

  if not state_file_exists then
    is_first_run = true
  elseif user_version == nil then
    -- Corrupted or manually edited state file: repair with safe defaults.
    write_state(current_version, nil)
    user_version = current_version
  elseif user_version < current_version then
    needs_update = true
  end

  if is_first_run then
    -- First run experience
    ensure_state_dir()

    local resource_dir = wezterm.executable_dir:gsub("MacOS/?$", "Resources")
    local first_run_script = resource_dir .. "/first_run.sh"

    -- Fallback for dev environment
    local f_script = io.open(first_run_script, "r")
    if not f_script then
      first_run_script = wezterm.executable_dir .. "/../../assets/shell-integration/first_run.sh"
    else
      f_script:close()
    end

    wezterm.mux.spawn_window {
      args = { 'bash', first_run_script },
      width = 106,
      height = 22,
    }
    return
  end

  if needs_update then
    -- Apply incremental config updates on version upgrades
    local resource_dir = wezterm.executable_dir:gsub("MacOS/?$", "Resources")
    local update_script = resource_dir .. "/check_config_version.sh"

    -- Fallback for dev environment
    local u_script = io.open(update_script, "r")
    if not u_script then
      update_script = wezterm.executable_dir .. "/../../assets/shell-integration/check_config_version.sh"
    else
      u_script:close()
    end

    wezterm.mux.spawn_window {
      args = { 'bash', update_script },
      width = 106,
      height = 22,
    }
    return
  end

  -- Normal startup
  if not cmd then
    local start_cwd = nil
    if remember_last_cwd then
      local saved = read_last_cwd()
      if saved and saved ~= '' then
        local result = os.execute(string.format('[ -d %q ] 2>/dev/null', saved))
        if result == true or result == 0 then
          start_cwd = saved
        end
      end
    end
    wezterm.mux.spawn_window(start_cwd and { cwd = start_cwd } or {})
  end
end)

return config
