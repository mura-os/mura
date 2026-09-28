//! `zwlr_layer_shell_v1` served (spec §4 rev 3.12; research/77 §2.1–2.2, §2.5): a layer surface
//! is a member of band 2/4/5 (background: band 1, accepted and not composed) on its frame's
//! place; arrange runs on its commit, map and unmap; the initial configure is sent on its first
//! commit **after** arranging (smithay's stated rule, `references/smithay/src/desktop/wayland/layer.rs:414-424`;
//! niri's order `references/niri/src/handlers/layer_shell.rs:103-210`); it maps on its first buffer
//! like the xdg path. Popups on a layer surface are tracked by the one `PopupManager` and
//! unconstrained to the frame rectangle (sway's full-output rule on the frame).

use smithay::backend::renderer::utils::with_renderer_surface_state;
use smithay::desktop::{LayerSurface, PopupKind};
use smithay::reexports::wayland_server::protocol::wl_output::WlOutput;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Resource;
use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::wlr_layer::{Layer, LayerSurface as WlrLayerSurface, LayerSurfaceData, WlrLayerShellHandler, WlrLayerShellState};
use smithay::wayland::shell::xdg::PopupSurface;

use super::{anchoring, arrange, band_of, LayerEntry, Surface};
use crate::input::focus;
use crate::scene::{Flags, Shape};
use crate::state::{Payload, Zxr};
use crate::xr::math;

impl WlrLayerShellHandler for Zxr {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(&mut self, surface: WlrLayerSurface, _output: Option<WlOutput>, layer: Layer, namespace: String) {
        let trusted = surface.wl_surface().client().map(|c| super::filter::is_trusted(&c)).unwrap_or(false);
        let ls = LayerSurface::new(surface, namespace.clone());
        // its own place on the head frame (re-parented by arrange when a row or request names
        // another frame), in the layer's band
        let place = self.scene.add_place(self.scene.head, math::pose_identity(), band_of(layer));
        let payload = Payload { window: Surface::Layer(ls.clone()), panel: None, dirty: false, mapped_at: 0, last_frame_callback: 0, hidden: false, urgent: false, requested_at_commit: self.focus.last_commit_serial, pending_activation: None, trusted };
        // `Scene::add` focuses the new member; a layer surface never takes focus by being created
        let prev = self.scene.focused;
        let id = self.scene.add(place, math::pose_identity(), Shape::Plane { size: [0.01, 0.01] }, Flags(0), payload).expect("place is live");
        self.scene.focus(prev);
        let serial = self.shell.next_serial();
        self.shell.layers.push(LayerEntry { member: id, surface: ls, namespace: namespace.clone(), serial, frame: anchoring::Frame::Head, box_px: None, mapped: false, mapped_at_serial: 0 });
        self.journal.layer_surfaces += 1;
        tracing::info!(member = id.0.index(), ?layer, %namespace, trusted, "layer surface created");
    }

    fn new_popup(&mut self, _parent: WlrLayerSurface, popup: PopupSurface) {
        self.unconstrain_popup(&popup);
        let _ = self.popups.track_popup(PopupKind::Xdg(popup));
        self.journal.popups += 1;
    }

    fn layer_destroyed(&mut self, surface: WlrLayerSurface) {
        let Some(i) = self.shell.layers.iter().position(|e| e.surface.layer_surface() == &surface) else { return };
        let entry = self.shell.layers.remove(i);
        let id = entry.member;
        let had_focus = self.scene.focused == Some(id);
        if let Some(member) = self.scene.remove(id) {
            if member.m.mapped() {
                self.journal.layer_unmapped += 1;
            }
            if let Some(panel) = member.m.panel {
                self.retire_panel(panel);
            }
            self.scene.remove_place(member.place);
        }
        tracing::info!(member = id.0.index(), ns = %entry.namespace, "layer surface destroyed");
        arrange(self);
        if had_focus {
            focus::restore_after_close(self, id);
        } else {
            self.focus.stack.remove(id);
            focus::layer_focus_changed(self);
        }
    }
}

/// The layer surface's commit (called from `CompositorHandler::commit` for a member whose
/// surface is a layer surface): anchoring state applies, arrange runs, the initial configure
/// goes out on the first commit, the first buffer maps, a null buffer unmaps.
pub fn commit(st: &mut Zxr, id: crate::scene::MemberId, surface: &WlSurface) {
    let Some(entry) = st.shell.entry(id) else { return };
    let ls = entry.surface.clone();
    let root = ls.wl_surface().clone();
    if &root != surface {
        // a subsurface or a popup of the layer surface: the panel redraws, nothing rearranges
        return;
    }
    let _anchoring_changed = anchoring::apply_pending(&root);
    let has_buffer = with_renderer_surface_state(&root, |s| s.buffer().is_some()).unwrap_or(false);
    let was_mapped = st.shell.entry(id).map(|e| e.mapped).unwrap_or(false);
    if !has_buffer && was_mapped {
        // unmapped by a null buffer: back to the post-`get_layer_surface` state (protocol
        // `:113-119`). Read before the initial-configure test: smithay resets the role on this
        // very commit (`wlr_layer/mod.rs` `got_unmapped` → `reset()`), so the test would otherwise
        // take the unmap for the client's next initial commit and configure it too early. The
        // configure follows the client's own re-initial commit, as the protocol says.
        if let Some(e) = st.shell.entry_mut(id) {
            e.mapped = false;
            e.box_px = None;
        }
        if let Some(m) = st.scene.get_mut(id) {
            m.m.mapped_at = 0;
        }
        st.journal.layer_unmapped += 1;
        let had_focus = st.scene.focused == Some(id);
        arrange(st);
        if had_focus {
            focus::restore_after_close(st, id);
        }
        return;
    }
    let initial_sent = with_states(&root, |states| states.data_map.get::<LayerSurfaceData>().map(|d| d.lock().unwrap().initial_configure_sent).unwrap_or(false));
    if !initial_sent {
        // arrange first so the configure carries the arranged size (the client's own size
        // respected where it set one), then the initial configure — research/77 §2.2
        arrange(st);
        ls.layer_surface().send_configure();
        st.shell.configures += 1;
        st.journal.layer_configures += 1;
        return;
    }
    if has_buffer && !was_mapped {
        let serial = st.shell.next_serial();
        if let Some(e) = st.shell.entry_mut(id) {
            e.mapped = true;
            e.mapped_at_serial = serial;
        }
        if let Some(m) = st.scene.get_mut(id) {
            m.m.mapped_at = st.frame_id.max(1);
        }
        st.journal.layer_mapped += 1;
        arrange(st);
        // on map: an `on_demand` surface on top/overlay enters the stack by the new-window rule
        // (cosmic-comp `map_layer`, niri's marker; research/77 §2.3); `exclusive` is the override
        // arrange just recomputed; `none` never
        let s = ls.cached_state();
        if s.keyboard_interactivity == smithay::wayland::shell::wlr_layer::KeyboardInteractivity::OnDemand && matches!(s.layer, Layer::Top | Layer::Overlay) {
            let at_request = st.scene.get(id).and_then(|m| m.m.requested_at_commit);
            if focus::new_window_takes_focus(at_request, st.focus.last_commit_serial) {
                st.focus.stack.touch(id);
                st.focus.new_windows_focused += 1;
                st.focus_window(Some(id));
            }
        }
    } else if was_mapped {
        // a mapped commit: state may have moved (size, anchors, zone, layer, anchoring)
        let band = band_of(ls.cached_state().layer);
        if let Some(pl) = st.scene.get(id).map(|m| m.place) {
            st.scene.set_place_band(pl, band);
        }
        arrange(st);
    }
}
