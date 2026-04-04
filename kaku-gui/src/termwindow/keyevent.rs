use crate::termwindow::InputMap;
use ::window::{
    DeadKeyStatus, KeyCode, KeyEvent, KeyboardLedStatus, Modifiers, RawKeyEvent, WindowOps,
};
use config::keyassignment::{KeyAssignment, KeyTableEntry};
use mux::pane::{Pane, PaneId, PerformAssignmentResult};
use smol::Timer;
use std::sync::Arc;
use std::time::{Duration, Instant};
use termwiz::input::KeyboardEncoding;

#[derive(Debug, Clone)]
pub struct KeyTableStateEntry {
    name: String,
    /// If this activation expires, when it should expire
    expiration: Option<Instant>,
    /// Whether this activation pops itself after recognizing a key press
    one_shot: bool,
    until_unknown: bool,
    prevent_fallback: bool,
    /// The timeout duration; used when updating the expiration
    timeout_milliseconds: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct KeyTableArgs<'a> {
    pub name: &'a str,
    pub timeout_milliseconds: Option<u64>,
    pub replace_current: bool,
    pub one_shot: bool,
    pub until_unknown: bool,
    pub prevent_fallback: bool,
}

#[derive(Debug, Default, Clone)]
pub struct KeyTableState {
    stack: Vec<KeyTableStateEntry>,
}

impl KeyTableState {
    pub fn activate(&mut self, args: KeyTableArgs) {
        if args.replace_current {
            self.pop();
        }
        self.stack.push(KeyTableStateEntry {
            name: args.name.to_string(),
            expiration: args
                .timeout_milliseconds
                .map(|ms| Instant::now() + Duration::from_millis(ms)),
            one_shot: args.one_shot,
            until_unknown: args.until_unknown,
            prevent_fallback: args.prevent_fallback,
            timeout_milliseconds: args.timeout_milliseconds,
        });
    }

    pub fn pop(&mut self) {
        self.stack.pop();
    }

    pub fn clear_stack(&mut self) {
        self.stack.clear();
    }

    pub fn process_expiration(&mut self) -> bool {
        let should_pop = self
            .stack
            .last()
            .map(|entry| match entry.expiration {
                Some(deadline) => Instant::now() >= deadline,
                None => false,
            })
            .unwrap_or(false);
        if !should_pop {
            return false;
        }
        self.pop();
        true
    }

    pub fn pop_until_unknown(&mut self) {
        while self
            .stack
            .last()
            .map(|entry| entry.until_unknown)
            .unwrap_or(false)
        {
            self.pop();
        }
    }

    pub fn current_table(&mut self) -> Option<&str> {
        while self.process_expiration() {}
        self.stack.last().map(|entry| entry.name.as_str())
    }

    fn lookup_key(
        &mut self,
        input_map: &InputMap,
        key: &KeyCode,
        mods: Modifiers,
        only_key_bindings: OnlyKeyBindings,
    ) -> Option<(KeyTableEntry, Option<String>)> {
        while self.process_expiration() {}

        let mut pop_count = 0;
        let mut result = None;

        for stack_entry in self.stack.iter_mut().rev() {
            let name = stack_entry.name.as_str();
            if let Some(entry) = input_map.lookup_key(key, mods, Some(name)) {
                if let Some(timeout) = stack_entry.timeout_milliseconds {
                    stack_entry
                        .expiration
                        .replace(Instant::now() + Duration::from_millis(timeout));
                }
                result = Some((entry, Some(name.to_string())));
                break;
            }

            if stack_entry.until_unknown {
                pop_count += 1;
            }

            if stack_entry.prevent_fallback {
                // If we've passed the key-bindings-only phase, then we want
                // to prevent the default action of passing the key through.
                // Prior to that, we mustn't prevent subsequent phases.
                if only_key_bindings == OnlyKeyBindings::No {
                    result = Some((
                        KeyTableEntry {
                            action: KeyAssignment::Nop,
                        },
                        Some(name.to_string()),
                    ));
                }

                // Whether we explicitly map Nop or not, prevent looking
                // in later key tables on the stack.
                break;
            }
        }

        // This is a little bit tricky: until_unknown needs to
        // pop entries if we didn't match, but since we need to
        // make three separate passes to resolve a key using its
        // various physical, mapped and raw forms, we cannot
        // unilaterally pop here without breaking a later pass.
        // It is only safe to pop here if we did match something:
        // in that case we know that we won't make additional
        // passes.
        // It is important that `pop_until_unknown` is called
        // in the final "no keys matched" case to correctly
        // manage that state transition.
        if result.is_some() {
            for _ in 0..pop_count {
                self.pop();
            }
        }

        result
    }

    pub fn did_process_key(&mut self) {
        let should_pop = self
            .stack
            .last()
            .map(|entry| entry.one_shot)
            .unwrap_or(false);
        if should_pop {
            self.pop();
        }
    }
}

#[derive(Debug)]
pub enum Key {
    Code(::termwiz::input::KeyCode),
    Composed(String),
    None,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum OnlyKeyBindings {
    Yes,
    No,
}

impl super::TermWindow {
    pub(super) fn clear_line_editor_selection(&mut self) {
        self.line_editor_selection = super::LineEditorSelectionState::None;
        self.line_editor_selection_owner = None;
    }

    fn has_line_editor_selection(&self) -> bool {
        self.line_editor_selection != super::LineEditorSelectionState::None
    }

    fn ensure_line_editor_selection_for_pane(&mut self, pane_id: PaneId) {
        if self.has_line_editor_selection() && self.line_editor_selection_owner != Some(pane_id) {
            self.clear_line_editor_selection();
        }
    }

    fn sync_line_editor_selection_owner(&mut self, pane_id: PaneId) {
        self.line_editor_selection_owner = if self.has_line_editor_selection() {
            Some(pane_id)
        } else {
            None
        };
    }

    fn extend_line_editor_charwise(&mut self, direction: super::LineEditorSelectionDirection) {
        use super::LineEditorSelectionState as Sel;

        self.line_editor_selection = match self.line_editor_selection {
            Sel::None => Sel::Charwise {
                direction,
                count: 1,
            },
            Sel::Charwise {
                direction: existing,
                count,
            } if existing == direction => Sel::Charwise {
                direction,
                count: count.saturating_add(1),
            },
            Sel::Charwise {
                direction: existing,
                count,
            } => {
                if count > 1 {
                    Sel::Charwise {
                        direction: existing,
                        count: count - 1,
                    }
                } else {
                    Sel::None
                }
            }
            Sel::ToStart | Sel::ToEnd | Sel::All | Sel::Unknown => Sel::Unknown,
        };
    }

    fn bytes_for_line_editor_selection_delete(&self) -> Option<Vec<u8>> {
        use super::{LineEditorSelectionDirection as Dir, LineEditorSelectionState as Sel};

        let bytes = match self.line_editor_selection {
            Sel::None => return None,
            Sel::Charwise {
                direction: Dir::Left,
                count,
            } => vec![0x04; count], // Cursor is at left edge of the region; delete forward.
            Sel::Charwise {
                direction: Dir::Right,
                count,
            } => vec![0x7f; count], // Cursor is at right edge of the region; delete backward.
            Sel::ToStart => vec![0x18, 0x18, 0x15], // C-x C-x, C-u
            Sel::ToEnd => vec![0x18, 0x18, 0x0b],   // C-x C-x, C-k
            Sel::All => vec![0x15],                 // C-u
            Sel::Unknown => return None,
        };
        Some(bytes)
    }

    pub(super) fn encode_win32_input(
        &self,
        pane: &Arc<dyn Pane>,
        key: &KeyEvent,
    ) -> Option<String> {
        if !self.config.allow_win32_input_mode
            || pane.get_keyboard_encoding() != KeyboardEncoding::Win32
        {
            return None;
        }
        key.encode_win32_input_mode()
    }

    pub(super) fn encode_kitty_input(
        &self,
        pane: &Arc<dyn Pane>,
        key: &KeyEvent,
    ) -> Option<String> {
        if !self.config.enable_kitty_keyboard {
            return None;
        }
        if let KeyboardEncoding::Kitty(flags) = pane.get_keyboard_encoding() {
            Some(key.encode_kitty(flags))
        } else {
            None
        }
    }

    fn lookup_key(
        &mut self,
        pane: &Arc<dyn Pane>,
        keycode: &KeyCode,
        mods: Modifiers,
        only_key_bindings: OnlyKeyBindings,
    ) -> Option<(KeyTableEntry, Option<String>)> {
        if let Some(overlay) = self.pane_state(pane.pane_id()).overlay.as_mut() {
            if let Some((entry, table_name)) = overlay.key_table_state.lookup_key(
                &self.input_map,
                keycode,
                mods,
                only_key_bindings,
            ) {
                return Some((entry, table_name.map(|s| s.to_string())));
            }
        }
        if let Some((entry, table_name)) =
            self.key_table_state
                .lookup_key(&self.input_map, keycode, mods, only_key_bindings)
        {
            return Some((entry, table_name.map(|s| s.to_string())));
        }
        self.input_map
            .lookup_key(keycode, mods, None)
            .map(|entry| (entry, None))
    }

    fn process_key(
        &mut self,
        pane: &Arc<dyn Pane>,
        context: &dyn WindowOps,
        keycode: &KeyCode,
        raw_modifiers: Modifiers,
        leader_active: bool,
        leader_mod: Modifiers,
        only_key_bindings: OnlyKeyBindings,
        is_down: bool,
        key_event: Option<&KeyEvent>,
    ) -> bool {
        if is_down && !leader_active {
            // Check to see if this key-press is the leader activating
            if let Some(duration) = self.input_map.is_leader(&keycode, raw_modifiers) {
                // Yes; record its expiration
                let target = std::time::Instant::now() + duration;
                self.leader_is_down.replace(target);
                self.update_title();
                // schedule an invalidation so that the cursor or status
                // area will be repainted at the right time
                if let Some(window) = self.window.clone() {
                    promise::spawn::spawn(async move {
                        Timer::at(target).await;
                        window.invalidate();
                    })
                    .detach();
                }
                return true;
            }
        }

        if is_down {
            if let Some((entry, table_name)) = self.lookup_key(
                pane,
                &keycode,
                raw_modifiers | leader_mod,
                only_key_bindings,
            ) {
                if self.config.debug_key_events {
                    log::info!(
                        "{}{:?} {:?} -> perform {:?}",
                        match table_name {
                            Some(name) => format!("table:{} ", name),
                            None => String::new(),
                        },
                        keycode,
                        raw_modifiers | leader_mod,
                        entry.action,
                    );
                }

                self.key_table_state.did_process_key();
                let handled = match self.perform_key_assignment(&pane, &entry.action) {
                    Ok(PerformAssignmentResult::Handled) => true,
                    Err(e) => {
                        log::warn!("perform_key_assignment failed: {:?}", e);
                        true
                    }
                    Ok(_) => false,
                };

                if handled {
                    context.invalidate();

                    if leader_active {
                        // A successful leader key-lookup cancels the leader
                        // virtual modifier state
                        self.leader_done();
                    }

                    return true;
                }
            }
        }

        // While the leader modifier is active, only registered
        // keybindings are recognized.
        let only_key_bindings = match (only_key_bindings, leader_active) {
            (OnlyKeyBindings::Yes, _) => OnlyKeyBindings::Yes,
            (_, true) => OnlyKeyBindings::Yes,
            _ => OnlyKeyBindings::No,
        };

        if only_key_bindings == OnlyKeyBindings::No {
            let config = &self.config;

            // This is a bit ugly.
            // Not all of our platforms report LEFT|RIGHT ALT; most report just ALT.
            // For those that do distinguish between them we want to respect the left vs.
            // right settings for the compose behavior.
            // Otherwise, if the event didn't include left vs. right then we want to
            // respect the generic compose behavior.
            let bypass_compose =
                    // Left ALT and they disabled compose
                    (raw_modifiers.contains(Modifiers::LEFT_ALT)
                    && !config.send_composed_key_when_left_alt_is_pressed)
                    // Right ALT and they disabled compose
                    || (raw_modifiers.contains(Modifiers::RIGHT_ALT)
                        && !config.send_composed_key_when_right_alt_is_pressed)
                    // Generic ALT and they disabled generic compose
                    || (!raw_modifiers.contains(Modifiers::RIGHT_ALT)
                        && !raw_modifiers.contains(Modifiers::LEFT_ALT)
                        && raw_modifiers.contains(Modifiers::ALT)
                        && !(config.send_composed_key_when_left_alt_is_pressed
                             || config.send_composed_key_when_right_alt_is_pressed));

            if bypass_compose {
                if let Key::Code(term_key) = self.win_key_code_to_termwiz_key_code(keycode) {
                    let tw_raw_modifiers = raw_modifiers;

                    if self.config.debug_key_events {
                        log::info!(
                            "{:?} {:?} -> send to pane {:?} {:?}",
                            keycode,
                            raw_modifiers,
                            term_key,
                            tw_raw_modifiers
                        );
                    }

                    let did_encode = self
                        .send_terminal_input_key(
                            pane,
                            term_key,
                            tw_raw_modifiers,
                            is_down,
                            key_event,
                        )
                        .is_ok();

                    if did_encode {
                        if is_down
                            && !keycode.is_modifier()
                            && self.pane_state(pane.pane_id()).overlay.is_none()
                        {
                            self.maybe_scroll_to_bottom_for_input(&pane);
                        }
                        if is_down
                            && self.config.hide_mouse_cursor_when_typing
                            && !keycode.is_modifier()
                        {
                            context.set_cursor(None);
                        }
                        if !keycode.is_modifier() {
                            context.invalidate();
                        }

                        return true;
                    }
                }
            }
        }

        false
    }

    pub fn raw_key_event_impl(&mut self, key: RawKeyEvent, context: &dyn WindowOps) {
        // The leader key is a kind of modal modifier key.
        // It is allowed to be active for up to the leader timeout duration,
        // after which it auto-deactivates.
        let (leader_active, leader_mod) = if self.leader_is_active_mut() {
            // Currently active
            (true, Modifiers::LEADER)
        } else {
            (false, Modifiers::NONE)
        };

        if self.config.debug_key_events {
            log::info!(
                "key_event {:?} {}",
                key,
                if leader_active { "LEADER" } else { "" }
            );
        } else {
            log::trace!(
                "key_event {:?} {}",
                key,
                if leader_active { "LEADER" } else { "" }
            );
        }

        let modifier_and_leds = (key.modifiers, key.leds);
        if self.current_modifier_and_leds != modifier_and_leds {
            self.current_modifier_and_leds = modifier_and_leds;
            self.schedule_next_status_update();
        }
        // On macOS, RawKeyEvent is delivered before KeyEvent. If a modal is open,
        // ignore raw-path keybinding processing so text cannot leak into the pane.
        // KeyEvent handling will dispatch to the modal immediately after this.
        if self.get_modal().is_some() {
            return;
        }

        let pane = match self.get_active_pane_or_overlay() {
            Some(pane) => pane,
            None => return,
        };

        if key.key_is_down
            && self.pane_state(pane.pane_id()).overlay.is_none()
            && !leader_active
            && !self.should_preserve_line_editor_selection_for_raw_key(&key)
        {
            self.clear_line_editor_selection();
        }

        // First, try to match raw physical key
        let phys_key = match &key.key {
            phys @ KeyCode::Physical(_) => Some(phys.clone()),
            _ => key.phys_code.map(KeyCode::Physical),
        };

        if let Some(phys_key) = &phys_key {
            if self.process_key(
                &pane,
                context,
                &phys_key,
                key.modifiers,
                leader_active,
                leader_mod,
                OnlyKeyBindings::Yes,
                key.key_is_down,
                None,
            ) {
                self.sync_raw_line_editor_selection_after_shortcut(
                    pane.pane_id(),
                    &key,
                    leader_active,
                );
                key.set_handled();
                return;
            }
        }

        // Then try the raw code
        let raw_key = match &key.key {
            raw @ KeyCode::RawCode(_) => raw.clone(),
            _ => KeyCode::RawCode(key.raw_code),
        };
        if self.process_key(
            &pane,
            context,
            &raw_key,
            key.modifiers,
            leader_active,
            leader_mod,
            OnlyKeyBindings::Yes,
            key.key_is_down,
            None,
        ) {
            self.sync_raw_line_editor_selection_after_shortcut(pane.pane_id(), &key, leader_active);
            key.set_handled();
            return;
        }

        if phys_key.as_ref() == Some(&key.key) || raw_key == key.key {
            // We already matched against whatever key.key is, so no need
            // to do it again below
            return;
        }

        if self.process_key(
            &pane,
            context,
            &key.key,
            key.modifiers,
            leader_active,
            leader_mod,
            OnlyKeyBindings::Yes,
            key.key_is_down,
            None,
        ) {
            self.sync_raw_line_editor_selection_after_shortcut(pane.pane_id(), &key, leader_active);
            key.set_handled();
        }
    }

    pub fn current_modifier_and_led_state(&self) -> (Modifiers, KeyboardLedStatus) {
        self.current_modifier_and_leds
    }

    pub fn leader_is_active(&self) -> bool {
        match self.leader_is_down.as_ref() {
            Some(expiry) if *expiry > std::time::Instant::now() => {
                self.update_next_frame_time(Some(*expiry));
                true
            }
            Some(_) => false,
            None => false,
        }
    }

    pub fn leader_is_active_mut(&mut self) -> bool {
        match self.leader_is_down.as_ref() {
            Some(expiry) if *expiry > std::time::Instant::now() => {
                self.update_next_frame_time(Some(*expiry));
                true
            }
            Some(_) => {
                self.leader_done();
                false
            }
            None => false,
        }
    }

    pub fn current_key_table_name(&mut self) -> Option<String> {
        let mut name = None;

        if let Some(pane) = self.get_active_pane_or_overlay() {
            if let Some(overlay) = self.pane_state(pane.pane_id()).overlay.as_mut() {
                name = overlay
                    .key_table_state
                    .current_table()
                    .map(|s| s.to_string());

                if let Some(entry) = overlay.key_table_state.stack.last() {
                    if let Some(expiry) = entry.expiration {
                        self.update_next_frame_time(Some(expiry));
                    }
                }
            }
        }
        if name.is_none() {
            name = self.key_table_state.current_table().map(|s| s.to_string());
        }
        if let Some(entry) = self.key_table_state.stack.last() {
            if let Some(expiry) = entry.expiration {
                self.update_next_frame_time(Some(expiry));
            }
        }
        name
    }

    pub fn composition_status(&self) -> &DeadKeyStatus {
        &self.dead_key_status
    }

    fn leader_done(&mut self) {
        self.leader_is_down.take();
        self.update_title();
        if let Some(window) = &self.window {
            window.invalidate();
        }
    }

    fn maybe_handle_native_line_editor_shortcut(
        &mut self,
        pane: &Arc<dyn Pane>,
        window_key: &KeyEvent,
        context: &dyn WindowOps,
    ) -> bool {
        use super::{LineEditorSelectionDirection as Dir, LineEditorSelectionState as Sel};
        use ::window::KeyCode as WK;

        if !window_key.key_is_down {
            return false;
        }

        self.ensure_line_editor_selection_for_pane(pane.pane_id());

        let mods = window_key.modifiers.remove_positional_mods();
        let mut out: Vec<u8> = Vec::new();
        let mut handled = true;
        let mut keep_selection_anchor = false;

        // Note: We intentionally do NOT set the shell mark here.
        // The shell-side selection widgets in setup_zsh.sh handle mark management
        // directly via zle, avoiding terminal encoding issues with NUL or CSI sequences.

        match (&window_key.key, mods) {
            (WK::LeftArrow | WK::ApplicationLeftArrow, Modifiers::SHIFT) => {
                // Send the Shift+Left CSI sequence so the shell-side
                // _kaku_select_left_char widget activates REGION_ACTIVE and
                // produces the visible selection highlight.
                out.extend_from_slice(b"\x1b[1;2D");
                self.extend_line_editor_charwise(Dir::Left);
                keep_selection_anchor = self.has_line_editor_selection();
            }
            (WK::RightArrow | WK::ApplicationRightArrow, Modifiers::SHIFT) => {
                out.extend_from_slice(b"\x1b[1;2C");
                self.extend_line_editor_charwise(Dir::Right);
                keep_selection_anchor = self.has_line_editor_selection();
            }
            (WK::LeftArrow | WK::ApplicationLeftArrow, m)
                if m == (Modifiers::SUPER | Modifiers::SHIFT) =>
            {
                // Send special sequence to invoke shell-side widget.
                out.extend_from_slice(b"\x1b[991~");
                self.line_editor_selection = Sel::ToStart;
                keep_selection_anchor = true;
            }
            (WK::RightArrow | WK::ApplicationRightArrow, m)
                if m == (Modifiers::SUPER | Modifiers::SHIFT) =>
            {
                out.extend_from_slice(b"\x1b[992~");
                self.line_editor_selection = Sel::ToEnd;
                keep_selection_anchor = true;
            }
            (WK::LeftArrow | WK::ApplicationLeftArrow, Modifiers::NONE)
                if self.has_line_editor_selection() =>
            {
                // Collapse selection leftward. The shell-side _kaku_mv_backward_char widget
                // (bound to ^B) deactivates REGION_ACTIVE before moving the cursor, so no
                // special escape sequence is needed here.
                out.push(0x02); // Ctrl-B (backward-char)
            }
            (WK::RightArrow | WK::ApplicationRightArrow, Modifiers::NONE)
                if self.has_line_editor_selection() =>
            {
                out.push(0x06); // Ctrl-F (forward-char)
            }
            (WK::LeftArrow | WK::ApplicationLeftArrow, Modifiers::SUPER)
                if self.has_line_editor_selection() =>
            {
                out.push(0x01); // Ctrl-A (beginning-of-line)
            }
            (WK::RightArrow | WK::ApplicationRightArrow, Modifiers::SUPER)
                if self.has_line_editor_selection() =>
            {
                out.push(0x05); // Ctrl-E (end-of-line)
            }
            (WK::Char('\u{1b}'), Modifiers::NONE) if self.has_line_editor_selection() => {
                // Cancel selection: clear GUI state and notify the shell to deactivate
                // its REGION_ACTIVE. We send a dedicated CSI sequence rather than bare
                // ESC, because ESC acts as a meta-prefix in zsh emacs mode and does not
                // clear the shell-side region highlight by itself.
                out.extend_from_slice(b"\x1b[995~");
                self.clear_line_editor_selection();
            }
            (WK::Char('\u{8}') | WK::Char('\u{7f}'), _) if self.has_line_editor_selection() => {
                if let Some(bytes) = self.bytes_for_line_editor_selection_delete() {
                    out.extend_from_slice(&bytes);
                    self.clear_line_editor_selection();
                    keep_selection_anchor = false;
                } else {
                    handled = false;
                }
            }
            _ => handled = false,
        }

        if !handled {
            self.clear_line_editor_selection();
            return false;
        }

        let res = self.write_terminal_input_bytes(pane, out.as_slice());
        if let Err(err) = res {
            log::warn!("{err:#}");
            self.clear_line_editor_selection();
            return false;
        }

        if !keep_selection_anchor {
            self.clear_line_editor_selection();
        } else {
            self.sync_line_editor_selection_owner(pane.pane_id());
        }
        self.maybe_scroll_to_bottom_for_input(pane);
        if self.config.hide_mouse_cursor_when_typing {
            context.set_cursor(None);
        }
        context.invalidate();

        if self.config.debug_key_events {
            log::info!(
                "native-line-editor shortcut key={:?} mods={:?} bytes={:?} selection={:?}",
                window_key.key,
                mods,
                out,
                self.line_editor_selection
            );
        }

        true
    }

    fn track_line_editor_selection_shortcut(&mut self, window_key: &KeyEvent) {
        use super::{LineEditorSelectionDirection as Dir, LineEditorSelectionState as Sel};
        use ::window::KeyCode as WK;

        if !window_key.key_is_down {
            return;
        }

        let mods = window_key.modifiers.remove_positional_mods();
        match (&window_key.key, mods) {
            (WK::LeftArrow | WK::ApplicationLeftArrow, Modifiers::SHIFT) => {
                self.extend_line_editor_charwise(Dir::Left);
            }
            (WK::RightArrow | WK::ApplicationRightArrow, Modifiers::SHIFT) => {
                self.extend_line_editor_charwise(Dir::Right);
            }
            (WK::LeftArrow | WK::ApplicationLeftArrow, m)
                if m == (Modifiers::SUPER | Modifiers::SHIFT) =>
            {
                self.line_editor_selection = Sel::ToStart;
            }
            (WK::RightArrow | WK::ApplicationRightArrow, m)
                if m == (Modifiers::SUPER | Modifiers::SHIFT) =>
            {
                self.line_editor_selection = Sel::ToEnd;
            }
            _ => {
                // Any other handled shortcut should not keep stale selection state.
                self.clear_line_editor_selection();
            }
        }
    }

    fn track_line_editor_selection_shortcut_raw(&mut self, key: &RawKeyEvent) {
        use super::LineEditorSelectionState as Sel;
        use ::window::PhysKeyCode as PK;

        if !key.key_is_down {
            return;
        }

        let mods = key.modifiers.remove_positional_mods();
        match (key.phys_code, mods) {
            (Some(PK::LeftArrow), m) if m == (Modifiers::SUPER | Modifiers::SHIFT) => {
                self.line_editor_selection = Sel::ToStart;
            }
            (Some(PK::RightArrow), m) if m == (Modifiers::SUPER | Modifiers::SHIFT) => {
                self.line_editor_selection = Sel::ToEnd;
            }
            _ => {
                self.clear_line_editor_selection();
            }
        }
    }

    fn sync_raw_line_editor_selection_after_shortcut(
        &mut self,
        pane_id: PaneId,
        key: &RawKeyEvent,
        leader_active: bool,
    ) {
        if self.pane_state(pane_id).overlay.is_none() && !leader_active {
            self.ensure_line_editor_selection_for_pane(pane_id);
            self.track_line_editor_selection_shortcut_raw(key);
            self.sync_line_editor_selection_owner(pane_id);
        }
    }

    /// Returns true for key categories that must not cancel an active line-editor selection.
    /// Plain-arrow and Cmd+arrow collapse cases are handled by early returns in the callers;
    /// this helper covers the remaining categories.
    fn key_category_preserves_selection(
        is_shift: bool,
        is_shift_arrow: bool,
        is_cmd_shift_arrow: bool,
        is_delete: bool,
        is_esc: bool,
    ) -> bool {
        is_shift || is_shift_arrow || is_cmd_shift_arrow || is_delete || is_esc
    }

    fn should_preserve_line_editor_selection_for_raw_key(&self, key: &RawKeyEvent) -> bool {
        use ::window::PhysKeyCode as PK;
        let m = key.modifiers.remove_positional_mods();
        let has_sel = self.has_line_editor_selection();

        // Plain Left/Right: if selection is active, preserve so maybe_handle_native_line_editor_shortcut
        // can collapse the region. Up/Down are excluded — they have no collapse arm and can be
        // pre-cleared immediately, which also avoids leaking stale zsh REGION_ACTIVE on history nav.
        // Also works around macOS modifier state desync where SHIFT may linger after release.
        if matches!(key.phys_code, Some(PK::LeftArrow | PK::RightArrow)) && m == Modifiers::NONE {
            return has_sel;
        }

        // Cmd+Arrow: same logic - if selection is active, let native shortcut handle it
        if matches!(key.phys_code, Some(PK::LeftArrow | PK::RightArrow)) && m == Modifiers::SUPER {
            return has_sel;
        }

        Self::key_category_preserves_selection(
            matches!(key.phys_code, Some(PK::LeftShift | PK::RightShift)),
            matches!(key.phys_code, Some(PK::LeftArrow | PK::RightArrow)) && m == Modifiers::SHIFT,
            matches!(key.phys_code, Some(PK::LeftArrow | PK::RightArrow))
                && m == (Modifiers::SUPER | Modifiers::SHIFT),
            matches!(key.phys_code, Some(PK::Backspace | PK::Delete)),
            // Preserve ESC in the raw path only when a selection is active, so the
            // subsequent key_event_impl handler can still cancel it.  Without this,
            // clear_line_editor_selection() would fire here first and the ESC cancel
            // branch in maybe_handle_native_line_editor_shortcut would never trigger.
            matches!(key.phys_code, Some(PK::Escape))
                && m == Modifiers::NONE
                && self.has_line_editor_selection(),
        )
    }

    fn should_preserve_line_editor_selection_for_key(&self, window_key: &KeyEvent) -> bool {
        use ::window::KeyCode as WK;
        let m = window_key.modifiers.remove_positional_mods();
        let k = &window_key.key;
        let has_sel = self.has_line_editor_selection();

        // Plain Left/Right: if selection is active, preserve so maybe_handle_native_line_editor_shortcut
        // can collapse the region. Up/Down are excluded — they have no collapse arm and can be
        // pre-cleared immediately, which also avoids leaking stale zsh REGION_ACTIVE on history nav.
        // Also works around macOS modifier state desync where SHIFT may linger after release.
        if matches!(
            k,
            WK::LeftArrow | WK::ApplicationLeftArrow | WK::RightArrow | WK::ApplicationRightArrow
        ) && m == Modifiers::NONE
        {
            return has_sel;
        }

        // Cmd+Arrow: same logic - if selection is active, let native shortcut handle it
        if matches!(
            k,
            WK::LeftArrow | WK::ApplicationLeftArrow | WK::RightArrow | WK::ApplicationRightArrow
        ) && m == Modifiers::SUPER
        {
            return has_sel;
        }

        Self::key_category_preserves_selection(
            matches!(k, WK::Shift | WK::LeftShift | WK::RightShift),
            matches!(
                k,
                WK::LeftArrow
                    | WK::ApplicationLeftArrow
                    | WK::RightArrow
                    | WK::ApplicationRightArrow
            ) && m == Modifiers::SHIFT,
            matches!(
                k,
                WK::LeftArrow
                    | WK::ApplicationLeftArrow
                    | WK::RightArrow
                    | WK::ApplicationRightArrow
            ) && m == (Modifiers::SUPER | Modifiers::SHIFT),
            matches!(k, WK::Char('\u{8}') | WK::Char('\u{7f}')),
            // ESC: only preserve when selection is active so the cancel branch in
            // maybe_handle_native_line_editor_shortcut can clear it.
            matches!(k, WK::Char('\u{1b}')) && m == Modifiers::NONE && has_sel,
        )
    }

    pub fn key_event_impl(&mut self, window_key: KeyEvent, context: &dyn WindowOps) {
        let pane = match self.get_active_pane_or_overlay() {
            Some(pane) => pane,
            None => return,
        };

        if window_key.key_is_down {
            let mut state = self.pane_state(pane.pane_id());
            if state.has_unread_bell {
                state.has_unread_bell = false;
                drop(state);
                crate::frontend::front_end().adjust_unread_bell_count(-1);
            }
        }

        // The leader key is a kind of modal modifier key.
        // It is allowed to be active for up to the leader timeout duration,
        // after which it auto-deactivates.
        let (leader_active, leader_mod) = if self.leader_is_active_mut() {
            // Currently active
            (true, Modifiers::LEADER)
        } else {
            (false, Modifiers::NONE)
        };

        if window_key.key_is_down
            && self.pane_state(pane.pane_id()).overlay.is_none()
            && !leader_active
            && !self.should_preserve_line_editor_selection_for_key(&window_key)
        {
            self.clear_line_editor_selection();
        }

        if self.config.debug_key_events {
            log::info!(
                "key_event {:?} {}",
                window_key,
                if leader_active { "LEADER" } else { "" }
            );
        } else {
            log::trace!(
                "key_event {:?} {}",
                window_key,
                if leader_active { "LEADER" } else { "" }
            );
        }

        let modifiers = window_key.modifiers;
        if let Some(modal) = self.get_modal() {
            if window_key.key_is_down {
                let modal_mods = modifiers.remove_positional_mods();
                match self.win_key_code_to_termwiz_key_code(&window_key.key) {
                    Key::Code(key) => {
                        if let Err(err) = modal.key_down(key, modal_mods, self) {
                            log::error!("Error dispatching key to modal: {err:#}");
                        }
                    }
                    Key::Composed(text) => {
                        let chars: Vec<char> = text.chars().collect();
                        for (idx, c) in chars.iter().enumerate() {
                            if let Err(err) = modal.key_down(
                                ::termwiz::input::KeyCode::Char(*c),
                                modal_mods,
                                self,
                            ) {
                                let remaining = chars.len().saturating_sub(idx + 1);
                                log::error!(
                                    "Error dispatching composed key '{}' to modal: {err:#}; \
                                     remaining {} character(s) in this composed sequence not dispatched",
                                    c,
                                    remaining
                                );
                                // Stop dispatching on first error to avoid cascading issues
                                // if the modal has entered an invalid state
                                break;
                            }
                        }
                    }
                    Key::None => {}
                }
            }
            return;
        }

        if self.process_key(
            &pane,
            context,
            &window_key.key,
            window_key.modifiers,
            leader_active,
            leader_mod,
            OnlyKeyBindings::No,
            window_key.key_is_down,
            Some(&window_key),
        ) {
            if self.pane_state(pane.pane_id()).overlay.is_none() && !leader_active {
                self.ensure_line_editor_selection_for_pane(pane.pane_id());
                self.track_line_editor_selection_shortcut(&window_key);
                self.sync_line_editor_selection_owner(pane.pane_id());
            }
            return;
        }

        // If we get here, then none of the keys matched
        // any key table rules. Therefore, we should pop all `until_unknown`
        // entries from the stack.
        if window_key.key_is_down {
            self.key_table_state.pop_until_unknown();
        }

        // Fallback for shell prompt line-editing habits on macOS.
        // We intentionally emit readline/zle emacs controls here so that
        // behavior works even when modified cursor CSI sequences are not bound.
        if self.pane_state(pane.pane_id()).overlay.is_none()
            && !leader_active
            && self.maybe_handle_native_line_editor_shortcut(&pane, &window_key, context)
        {
            return;
        }

        let key = self.win_key_code_to_termwiz_key_code(&window_key.key);

        match key {
            Key::Code(key) => {
                if window_key.key_is_down && !key.is_modifier() {
                    if leader_active {
                        // Leader was pressed and this non-modifier keypress isn't
                        // a registered key binding; swallow this event and cancel
                        // the leader modifier.
                        self.leader_done();
                        return;
                    }
                    self.key_table_state.did_process_key();
                }

                if self.config.debug_key_events {
                    log::info!(
                        "send to pane {} key={:?} mods={:?}",
                        if window_key.key_is_down { "DOWN" } else { "UP" },
                        key,
                        modifiers
                    );
                }

                let res = self.send_terminal_input_key(
                    &pane,
                    key,
                    modifiers,
                    window_key.key_is_down,
                    Some(&window_key),
                );

                if res.is_ok() {
                    if window_key.key_is_down
                        && !key.is_modifier()
                        && self.pane_state(pane.pane_id()).overlay.is_none()
                    {
                        self.clear_line_editor_selection();
                    }
                    if window_key.key_is_down
                        && !key.is_modifier()
                        && self.pane_state(pane.pane_id()).overlay.is_none()
                    {
                        self.maybe_scroll_to_bottom_for_input(&pane);
                    }
                    if window_key.key_is_down
                        && self.config.hide_mouse_cursor_when_typing
                        && !key.is_modifier()
                    {
                        context.set_cursor(None);
                    }
                    if !key.is_modifier() {
                        context.invalidate();
                    }
                }
            }
            Key::Composed(s) => {
                if !window_key.key_is_down {
                    return;
                }
                if leader_active {
                    // Leader was pressed and this non-modifier keypress isn't
                    // a registered key binding; swallow this event and cancel
                    // the leader modifier.
                    self.leader_done();
                    return;
                }
                self.key_table_state.did_process_key();
                if self.config.debug_key_events {
                    log::info!("send to pane string={:?}", s);
                }
                if let Err(err) = self.write_terminal_input_bytes(&pane, s.as_bytes()) {
                    log::warn!("sending composed input failed: {err:#}");
                }
                self.clear_line_editor_selection();
                self.maybe_scroll_to_bottom_for_input(&pane);
                context.invalidate();
            }
            Key::None => {}
        }
    }

    pub fn win_key_code_to_termwiz_key_code(&self, key: &::window::KeyCode) -> Key {
        use ::termwiz::input::KeyCode as KC;
        use ::window::KeyCode as WK;

        let code = match key {
            // TODO: consider eliminating these codes from termwiz::input::KeyCode
            WK::Char('\r') => KC::Enter,
            WK::Char('\t') => KC::Tab,
            WK::Char('\u{08}') => {
                if self.config.swap_backspace_and_delete {
                    KC::Delete
                } else {
                    KC::Backspace
                }
            }
            WK::Char('\u{7f}') => {
                if self.config.swap_backspace_and_delete {
                    KC::Backspace
                } else {
                    KC::Delete
                }
            }
            WK::Char('\u{1b}') => KC::Escape,
            WK::RawCode(_) => return Key::None,
            WK::Physical(phys) => {
                return self.win_key_code_to_termwiz_key_code(&phys.to_key_code());
            }

            WK::Char(c) => KC::Char(*c),
            WK::Composed(ref s) => {
                let mut chars = s.chars();
                if let Some(first_char) = chars.next() {
                    if chars.next().is_none() {
                        // Was just a single char after all
                        return self.win_key_code_to_termwiz_key_code(&WK::Char(first_char));
                    }
                }
                return Key::Composed(s.to_owned());
            }
            WK::Function(f) => KC::Function(*f),
            WK::LeftArrow => KC::LeftArrow,
            WK::RightArrow => KC::RightArrow,
            WK::UpArrow => KC::UpArrow,
            WK::DownArrow => KC::DownArrow,
            WK::Home => KC::Home,
            WK::End => KC::End,
            WK::PageUp => KC::PageUp,
            WK::PageDown => KC::PageDown,
            WK::Insert => KC::Insert,
            WK::Hyper => KC::Hyper,
            WK::Super => KC::Super,
            WK::Meta => KC::Meta,
            WK::Cancel => KC::Cancel,
            WK::Clear => KC::Clear,
            WK::Shift => KC::Shift,
            WK::LeftShift => KC::LeftShift,
            WK::RightShift => KC::RightShift,
            WK::Control => KC::Control,
            WK::LeftControl => KC::LeftControl,
            WK::RightControl => KC::RightControl,
            WK::Alt => KC::Alt,
            WK::LeftAlt => KC::LeftAlt,
            WK::RightAlt => KC::RightAlt,
            WK::Pause => KC::Pause,
            WK::CapsLock => KC::CapsLock,
            WK::VoidSymbol => return Key::None,
            WK::Select => KC::Select,
            WK::Print => KC::Print,
            WK::Execute => KC::Execute,
            WK::PrintScreen => KC::PrintScreen,
            WK::Help => KC::Help,
            WK::LeftWindows => KC::LeftWindows,
            WK::RightWindows => KC::RightWindows,
            WK::Sleep => KC::Sleep,
            WK::Multiply => KC::Multiply,
            WK::Applications => KC::Applications,
            WK::Add => KC::Add,
            WK::Numpad(0) => KC::Numpad0,
            WK::Numpad(1) => KC::Numpad1,
            WK::Numpad(2) => KC::Numpad2,
            WK::Numpad(3) => KC::Numpad3,
            WK::Numpad(4) => KC::Numpad4,
            WK::Numpad(5) => KC::Numpad5,
            WK::Numpad(6) => KC::Numpad6,
            WK::Numpad(7) => KC::Numpad7,
            WK::Numpad(8) => KC::Numpad8,
            WK::Numpad(9) => KC::Numpad9,
            WK::Numpad(_) => return Key::None,
            WK::Separator => KC::Separator,
            WK::Subtract => KC::Subtract,
            WK::Decimal => KC::Decimal,
            WK::Divide => KC::Divide,
            WK::NumLock => KC::NumLock,
            WK::ScrollLock => KC::ScrollLock,
            WK::Copy => KC::Copy,
            WK::Cut => KC::Cut,
            WK::Paste => KC::Paste,
            WK::BrowserBack => KC::BrowserBack,
            WK::BrowserForward => KC::BrowserForward,
            WK::BrowserRefresh => KC::BrowserRefresh,
            WK::BrowserStop => KC::BrowserStop,
            WK::BrowserSearch => KC::BrowserSearch,
            WK::BrowserFavorites => KC::BrowserFavorites,
            WK::BrowserHome => KC::BrowserHome,
            WK::VolumeMute => KC::VolumeMute,
            WK::VolumeDown => KC::VolumeDown,
            WK::VolumeUp => KC::VolumeUp,
            WK::MediaNextTrack => KC::MediaNextTrack,
            WK::MediaPrevTrack => KC::MediaPrevTrack,
            WK::MediaStop => KC::MediaStop,
            WK::MediaPlayPause => KC::MediaPlayPause,
            WK::ApplicationLeftArrow => KC::ApplicationLeftArrow,
            WK::ApplicationRightArrow => KC::ApplicationRightArrow,
            WK::ApplicationUpArrow => KC::ApplicationUpArrow,
            WK::ApplicationDownArrow => KC::ApplicationDownArrow,
            WK::KeyPadHome => KC::KeyPadHome,
            WK::KeyPadEnd => KC::KeyPadEnd,
            WK::KeyPadBegin => KC::KeyPadBegin,
            WK::KeyPadPageUp => KC::KeyPadPageUp,
            WK::KeyPadPageDown => KC::KeyPadPageDown,
        };
        Key::Code(code)
    }
}
