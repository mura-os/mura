//! `zwp_text_input_v3` client: the focused Slint text field's state to the compositor's input
//! method, and the input method's `commit_string` / `preedit_string` / `delete_surrounding_text`
//! back into Slint as composition events (`InternalKeyEvent`, the same path Slint's winit backend
//! uses for winit's `Ime` events — `winitwindowadapter.rs:1442-1460`). sctk 0.20 has no
//! text-input client, so this is a plain `Dispatch` on the generated protocol.
//!
//! squeekboard types through `commit_string` only (research/75 §3.2); preedit is supported for
//! input methods that use it.

use i_slint_core::input::{InternalKeyEvent, KeyEventType};
use i_slint_core::window::{InputMethodProperties, InputMethodRequest};
use slint::platform::WindowEvent;
use wayland_client::globals::GlobalList;
use wayland_client::protocol::{wl_seat, wl_surface};
use wayland_client::{delegate_noop, Connection, Dispatch, QueueHandle};
use wayland_protocols::wp::text_input::zv3::client::zwp_text_input_manager_v3::ZwpTextInputManagerV3;
use wayland_protocols::wp::text_input::zv3::client::zwp_text_input_v3::{self, ChangeCause, ContentHint, ContentPurpose, ZwpTextInputV3};

use crate::state::AppState;

pub(crate) struct TextInput {
    manager: Option<ZwpTextInputManagerV3>,
    object: Option<ZwpTextInputV3>,
    /// The compositor sent `enter` for our surface.
    active: bool,
    /// Slint has a focused text field with these properties.
    field: Option<InputMethodProperties>,
    enabled_sent: bool,
    pending_preedit: Option<(String, i32, i32)>,
    pending_commit: Option<String>,
    pending_delete: Option<(u32, u32)>,
    events: Vec<WindowEvent>,
}

impl TextInput {
    pub fn bind(globals: &GlobalList, qh: &QueueHandle<AppState>) -> Self {
        let manager = globals.bind::<ZwpTextInputManagerV3, AppState, ()>(qh, 1..=1, ()).ok();
        if manager.is_none() {
            tracing::info!("zwp_text_input_manager_v3 not offered; on-screen keyboard text will not reach fields");
        }
        TextInput { manager, object: None, active: false, field: None, enabled_sent: false, pending_preedit: None, pending_commit: None, pending_delete: None, events: Vec::new() }
    }

    pub fn seat_ready(&mut self, qh: &QueueHandle<AppState>, seat: &wl_seat::WlSeat) {
        if self.object.is_none() {
            if let Some(m) = &self.manager {
                self.object = Some(m.get_text_input(seat, qh, ()));
            }
        }
    }

    pub fn take_events(&mut self) -> Vec<WindowEvent> {
        std::mem::take(&mut self.events)
    }

    /// Slint's request (a field gained/changed/lost focus).
    pub fn handle_request(&mut self, request: InputMethodRequest, _scale: i32) {
        match request {
            InputMethodRequest::Enable(p) | InputMethodRequest::Update(p) => {
                self.field = Some(p);
                self.sync();
            }
            InputMethodRequest::Disable => {
                self.field = None;
                self.sync();
            }
            _ => {}
        }
    }

    /// Push our state to the compositor: enabled iff the surface has text-input focus and a field
    /// is focused.
    fn sync(&mut self) {
        let Some(ti) = &self.object else { return };
        match (&self.field, self.active) {
            (Some(p), true) => {
                ti.enable();
                let (hint, purpose) = content_type(p);
                ti.set_content_type(hint, purpose);
                ti.set_surrounding_text(p.text.to_string(), p.cursor_position as i32, p.anchor_position.unwrap_or(p.cursor_position) as i32);
                ti.set_text_change_cause(ChangeCause::Other);
                ti.set_cursor_rectangle(
                    p.cursor_rect_origin.x as i32,
                    p.cursor_rect_origin.y as i32,
                    p.cursor_rect_size.width.max(1.0) as i32,
                    p.cursor_rect_size.height.max(1.0) as i32,
                );
                ti.commit();
                self.enabled_sent = true;
            }
            _ => {
                if self.enabled_sent {
                    ti.disable();
                    ti.commit();
                    self.enabled_sent = false;
                }
            }
        }
    }

    fn event(&mut self, event: zwp_text_input_v3::Event, our_surface: Option<&wl_surface::WlSurface>) {
        match event {
            zwp_text_input_v3::Event::Enter { surface } => {
                if Some(&surface) == our_surface {
                    self.active = true;
                    self.sync();
                }
            }
            zwp_text_input_v3::Event::Leave { surface } => {
                if Some(&surface) == our_surface {
                    self.active = false;
                    self.enabled_sent = false;
                }
            }
            zwp_text_input_v3::Event::PreeditString { text, cursor_begin, cursor_end } => {
                self.pending_preedit = Some((text.unwrap_or_default(), cursor_begin, cursor_end));
            }
            zwp_text_input_v3::Event::CommitString { text } => {
                self.pending_commit = Some(text.unwrap_or_default());
            }
            zwp_text_input_v3::Event::DeleteSurroundingText { before_length, after_length } => {
                self.pending_delete = Some((before_length, after_length));
            }
            zwp_text_input_v3::Event::Done { serial: _ } => {
                // Protocol order: delete surrounding, commit, then the new preedit.
                if let Some((before, after)) = self.pending_delete.take() {
                    let (nb, na) = self.field.as_ref().map(|p| chars_around(&p.text, p.cursor_position, before as usize, after as usize)).unwrap_or((0, 0));
                    for _ in 0..nb {
                        self.events.push(WindowEvent::KeyPressed { text: slint::platform::Key::Backspace.into() });
                        self.events.push(WindowEvent::KeyReleased { text: slint::platform::Key::Backspace.into() });
                    }
                    for _ in 0..na {
                        self.events.push(WindowEvent::KeyPressed { text: slint::platform::Key::Delete.into() });
                        self.events.push(WindowEvent::KeyReleased { text: slint::platform::Key::Delete.into() });
                    }
                }
                if let Some(text) = self.pending_commit.take() {
                    let mut key_event = i_slint_core::input::KeyEvent::default();
                    key_event.text = text.into();
                    self.events.push(WindowEvent::internal(InternalKeyEvent { event_type: KeyEventType::CommitComposition, key_event, ..Default::default() }));
                }
                if let Some((text, begin, end)) = self.pending_preedit.take() {
                    let sel = (begin >= 0 && end >= 0).then_some(begin..end);
                    self.events.push(WindowEvent::internal(InternalKeyEvent {
                        event_type: KeyEventType::UpdateComposition,
                        preedit_text: text.into(),
                        preedit_selection: sel,
                        ..Default::default()
                    }));
                }
            }
            _ => {}
        }
    }
}

/// How many characters `before`/`after` bytes around the cursor cover.
fn chars_around(text: &str, cursor: usize, before: usize, after: usize) -> (usize, usize) {
    let cursor = cursor.min(text.len());
    let start = cursor.saturating_sub(before);
    let end = (cursor + after).min(text.len());
    let nb = text.get(start..cursor).map(|s| s.chars().count()).unwrap_or(0);
    let na = text.get(cursor..end).map(|s| s.chars().count()).unwrap_or(0);
    (nb, na)
}

fn content_type(p: &InputMethodProperties) -> (ContentHint, ContentPurpose) {
    use i_slint_core::items::InputType;
    match p.input_type {
        InputType::Password => (ContentHint::SensitiveData | ContentHint::HiddenText, ContentPurpose::Password),
        InputType::Number => (ContentHint::None, ContentPurpose::Digits),
        InputType::Decimal => (ContentHint::None, ContentPurpose::Number),
        _ => (ContentHint::None, ContentPurpose::Normal),
    }
}

impl Dispatch<ZwpTextInputV3, ()> for AppState {
    fn event(state: &mut Self, _: &ZwpTextInputV3, event: zwp_text_input_v3::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        let surface = state.our_surface().cloned();
        state.text_input.event(event, surface.as_ref());
    }
}

delegate_noop!(AppState: ignore ZwpTextInputManagerV3);
