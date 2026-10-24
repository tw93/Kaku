-- Run with: luajit scripts/test_yazi_launcher.lua [bundled-config-path]
-- Execute the actual launcher with fake panes/windows; never type into a real
-- terminal or run the theme wrapper against the user's configuration.
local path = arg[1] or 'assets/macos/Kaku.app/Contents/Resources/kaku.lua'
assert(loadfile(path)) -- Also guard the bundled chunk's local-variable limit.
local file = assert(io.open(path))
local source = file:read('*a')
file:close()
local first = assert(source:find('local function launch_yazi(window, pane)', 1, true))
local last = assert(source:find('\nlocal function evict_stale_cache', first, true))
local chunk = source:sub(first, last - 1)

local function fixture()
  local state = { launches = {}, toasts = {}, themes = 0, activations = {} }
  local env = setmetatable({ yazi_tabs = {} }, { __index = _G })
  env.resolve_active_pane = function(_, pane) return pane end
  env.resolve_yazi_command = function() return state.binary end
  env.resolve_remote_target_from_pane = function(pane) return pane.remote end
  env.pane_cwd = function(pane) return pane.cwd or '' end
  env.sync_managed_yazi_theme = function() state.themes = state.themes + 1 end
  env.show_yazi_toast = function(_, _, event) table.insert(state.toasts, event) end
  -- Allow the previous current-pane implementation to reach its unsafe input
  -- dispatch when checking this test against the unfixed bundled config.
  env.set_yazi_mode_hint = function() end
  env.update_window_config = function() end
  env.os = { getenv = function(name)
    assert(name == 'HOME')
    return '/local/home'
  end }
  -- Any accidental attempt to dispatch a key/action into the source pane fails.
  env.wezterm = { action = setmetatable({}, { __index = function()
    error('must not send input to the foreground application')
  end }) }
  state.binary = '/local/home/.config/kaku/zsh/bin/yazi'
  local launch, restore = assert(load(chunk .. '\nreturn launch_yazi, restore_yazi_tab',
    'yazi launcher', 't', env))()
  local function tab(id)
    return {
      tab_id = function() return id end,
      activate = function() table.insert(state.activations, id) end,
    }
  end
  local function window(id, source_id)
    local origin = tab(source_id)
    local mux = { live = { origin } }
    function mux:window_id() return id end
    function mux:tabs() return self.live end
    function mux:spawn_tab(spec)
      if state.fail_spawn then error('spawn failed') end
      table.insert(state.launches, spec)
      local child = tab(100 + #state.launches)
      table.insert(self.live, child)
      return child
    end
    return {
      mux_window = function() return mux end,
      active_tab = function() return origin end,
      get_dimensions = function() return { is_full_screen = false } end,
      perform_action = function() error('must not type into the source pane') end,
    }, mux, origin
  end
  return state, launch, restore, window, env
end

do
  local state, launch, restore, window, env = fixture()
  local win, mux, origin = window(1, 10)
  local pane = { cwd = '/tmp/project with spaces/中文', input = 'existing Codex prompt' }
  launch(win, pane)
  local spec = assert(state.launches[1], 'must spawn a tab from a Codex pane')
  assert(spec.cwd == pane.cwd)
  assert(spec.domain.DomainName == 'local')
  assert(#spec.args == 1 and spec.args[1] == state.binary)
  assert(pane.input == 'existing Codex prompt')
  assert(state.themes == 1 and #state.toasts == 0)
  assert(state.activations[1] == 101)
  restore(win)
  assert(#state.activations == 1, 'do not return while Yazi is still open')
  mux.live = { origin }
  restore(win)
  assert(state.activations[2] == 10)
  assert(next(env.yazi_tabs) == nil)
  restore(win)
  assert(#state.activations == 2, 'return only once')
end

do
  local state, launch, restore, window, env = fixture()
  local win, mux, origin = window(1, 10)
  local other = window(2, 20)
  launch(win, { cwd = '/tmp/project' })
  restore(other)
  assert(next(env.yazi_tabs), 'another window must not discard the return target')
  assert(#state.activations == 1)
  mux.live = { origin }
  restore(win)
  assert(state.activations[2] == 10)
end

do
  local state, launch, restore, window, env = fixture()
  local win, mux = window(1, 10)
  launch(win, { cwd = '/tmp/project' })
  mux.live = {} -- Both Yazi and its source were closed.
  restore(win)
  assert(#state.activations == 1 and next(env.yazi_tabs) == nil)
end

do
  local state, launch, _, window = fixture()
  local win = window(1, 10)
  launch(win, { cwd = '/remote/only', remote = 'user@server' })
  assert(state.launches[1].cwd == '/local/home')
  assert(state.launches[1].domain.DomainName == 'local')
  launch(win, {}) -- No cwd information: use a known local directory.
  assert(state.launches[2].cwd == '/local/home')
end

do
  local state, launch, _, window, env = fixture()
  local win = window(1, 10)
  launch(win, nil)
  assert(state.toasts[1] == 'kaku-toast-yazi-no-pane')
  state.binary = nil
  launch(win, {})
  assert(state.toasts[2] == 'kaku-toast-yazi-missing')
  assert(#state.launches == 0 and state.themes == 0)
  state.binary = '/missing/yazi'
  state.fail_spawn = true
  launch(win, {})
  assert(state.toasts[3] == 'kaku-toast-yazi-dispatch-failed')
  assert(next(env.yazi_tabs) == nil)
end

print('Yazi launcher regression tests passed')
