//! Animation Assist's bar, as the Android app's (ui/AnimationUi.kt): Play, Settings and Add Frame
//! over the timeline of frames, the current one in the middle; its settings (playback, frames a
//! second, onion skins, the onion skins' colours) or the current frame's options stand on it.
//! Page Assist shows the same bar with pages.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use egui::emath::TSTransform;
use egui::{Align2, Color32, Id, LayerId, Order, Pos2, Rect, Sense, Stroke, TextureHandle, Ui, pos2, vec2};
use procraft_core::{AnimationPlayback, AnimationSettings, CanvasSession, Palettes, animation_next_step};

use crate::colours::HarmonyKind;
use crate::compact_colours::{self, CompactColours};
use crate::icons;
use crate::look::{Look, Themed, Weight, argb, cubic_bezier, pc, text};
use crate::panel::{self, PANEL_TITLE};

/// The bar is 657 wide on a wide window, 446 upright; 96 tall, 25 over the bottom edge.
const WIDE_BAR: f32 = 657.0;
const UPRIGHT_BAR: f32 = 446.0;
const BAR_HEIGHT: f32 = 96.0;
const BAR_GAP: f32 = 25.0;
/// Play's and Add Frame's inset from the bar's ends.
const BAR_INSET: f32 = 26.0;
const FRAME_HEIGHT: f32 = 40.0;
const BAR_FILL: Themed = Themed(0xFF2A2A2A, 0xFFE4E4E4);
const TRACK_FILL: Themed = Themed(0xFF474747, 0xFFD0D0D0);
const TRACK_EDGE: Themed = Themed(0xFF5C5C5C, 0xFFBDBDBD);
const BAR_INK: Themed = Themed(0xFFC5C5C5, 0xFF393939);
const FRAME_EDGE: u32 = 0xFF9A9A9A;
const HELD_FRAME: u32 = 0xFFA3A3A3;
const POPOVER_FILL: Themed = Themed(0xFF383838, 0xFFE6E6E6);
const GROUP: Themed = Themed(0x420C0C0C, 0xFFE8E8E8);
const ROW_LINE: u32 = 0x09FFFFFF;
const SLIDE_LABEL: Themed = Themed(0xFFC1C1C1, 0xFF3D3D3D);
const SLIDE_VALUE: Themed = Themed(0xFFEDEDED, 0xFF3D3D3D);
const SLIDE_TRACK: Themed = Themed(0xFF414141, 0xFFBEBEBE);
const SWATCH_RIM: u32 = 0xFF969696;
const MAX_ONION: u32 = 12;
const MAX_HOLD: u32 = 60;
/// The frames' pictures on the timeline, this many pixels on their long side; read again at most
/// this often while the artwork changes.
const FRAME_THUMB_SIDE: u32 = 96;
const THUMBS_AGAIN: Duration = Duration::from_millis(250);
/// A frame slides into the middle in this long.
const SLIDE: f32 = 0.35;
/// A 23 pt title's line.
const TITLE_LINE: f32 = 28.0;
/// The compact picker on the colours page: drawn 260 wide, shown 282 wide, in a body this tall.
const PICKER_SHOWN: f32 = 282.0;
const PICKER_BODY: f32 = 365.0;

/// The timeline's motion: where the middle is (a frame index, fractional while moving).
#[derive(Default)]
struct Track {
    at: f32,
    /// Dragged by the pointer; the frame shown last as it passed the middle.
    dragging: bool,
    shown: Option<usize>,
    /// Sliding to a frame: from, to, since; and whether it ends a drag (the frame then becomes the
    /// current one).
    slide: Option<(f32, f32, Instant, bool)>,
    /// The frame it last settled on.
    focus: Option<usize>,
}

/// Playing: the frames in playing order, the place in it, the way it goes, and when the next
/// frame is due (none once a single pass has ended, its last frame still showing).
struct Playing {
    order: Vec<u32>,
    at: u32,
    step: i32,
    due: Option<Instant>,
}

#[derive(Default)]
pub struct AnimationBar {
    /// Whether a plugin draws in-betweens: the frame options offer In-between.
    pub tweens: bool,
    /// The plugins' actions for the frame options, and the one tapped there, to run (with that
    /// frame's layer current).
    pub frame_actions: Vec<procraft_core::PluginAction>,
    pub run_action: Option<String>,
    settings_open: bool,
    options_open: bool,
    /// The onion skins' colours page, editing the colour after the current frame (else before).
    colours: Option<(bool, CompactColours)>,
    playing: Option<Playing>,
    track: Track,
    /// The frames' pictures, and the edits and time they were read at.
    thumbs: HashMap<u64, TextureHandle>,
    thumbs_read: Option<(u64, usize, Instant)>,
    /// The frame and how far into its hold the timeline was last left, and its hold's slider held.
    held: (usize, usize),
    holding: bool,
}

/// Where frame `current` is among the timeline's `slots` (each frame's index, again for each frame
/// it is held), `held` being the frame and held place left on.
fn slot_of(slots: &[usize], current: usize, held: (usize, usize)) -> Option<usize> {
    let first = slots.iter().position(|f| *f == current)?;
    let places = slots.iter().filter(|f| **f == current).count();
    Some(first + if held.0 == current { held.1.min(places.saturating_sub(1)) } else { 0 })
}

/// What the bar did that the app shows (a message).
pub enum Said {
    NoRoom,
}

impl AnimationBar {
    /// The onion skins' colours have their hex keypad open: it takes the keys.
    pub fn takes_keys(&self) -> bool {
        self.colours.as_ref().is_some_and(|(_, c)| c.takes_keys())
    }

    /// Playing stops, menus close (Animation Assist went off, or a screen covers the bar).
    pub fn reset(&mut self, session: &CanvasSession) {
        self.stop(session);
        self.settings_open = false;
        self.options_open = false;
        self.colours = None;
    }

    fn stop(&mut self, session: &CanvasSession) {
        if self.playing.take().is_some() {
            session.set_play_frame(None);
        }
    }

    /// The frame showing while playing.
    fn playing_frame(&self) -> Option<usize> {
        let p = self.playing.as_ref()?;
        p.order.get(p.at as usize).map(|f| *f as usize)
    }

    /// Shows the bar while Animation or Page Assist is on.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        palettes: &Palettes,
        harmony: HarmonyKind,
    ) -> Option<Said> {
        let Some(settings) = session.animation_settings() else {
            if self.playing.is_some() || self.settings_open || self.options_open {
                self.reset(session);
            }
            return None;
        };
        let frames = session.animation_frames();
        let current = session.current_frame().map(|c| c as usize);
        let slots: Vec<usize> = session.animation_sequence().into_iter().map(|f| f as usize).collect();
        self.play_on(ctx, session, &settings);
        self.read_thumbs(ctx, session, &frames);
        let screen = ctx.content_rect();
        let width = if screen.width() > screen.height() { WIDE_BAR } else { UPRIGHT_BAR };
        let bar = Rect::from_min_size(
            pos2(screen.center().x - width / 2.0, screen.bottom() - BAR_GAP - BAR_HEIGHT),
            vec2(width, BAR_HEIGHT),
        );
        let pages = settings.pages;
        let mut said = None;
        let mut settings_at = 0.0;
        egui::Area::new(Id::unique("animation bar")).fixed_pos(bar.min).order(Order::Middle).show(ctx, |ui| {
            // the whole bar takes its presses: none reach the canvas under it
            ui.allocate_rect(bar, Sense::click_and_drag());
            ui.painter().rect_filled(bar, 30, argb(BAR_FILL));
            let y = bar.top() + 8.0 + 15.0;
            let ink = argb(BAR_INK);
            let play = if pages {
                "Page Assist"
            } else if self.playing.is_some() {
                "Pause"
            } else {
                "Play"
            };
            let shown =
                text(ui.painter(), pos2(bar.left() + BAR_INSET, y), Align2::LEFT_CENTER, play, 17.0, Weight::Bold, ink);
            let tapped = |ui: &Ui, r: Rect, id: &str| {
                let row = Rect::from_min_max(pos2(r.left(), y - 15.0), pos2(r.right(), y + 15.0));
                ui.interact(row, ui.scope_id().with(id), Sense::click()).clicked()
            };
            if !pages && tapped(ui, shown, "play") {
                if self.playing.is_some() {
                    self.stop(session);
                } else {
                    self.play(ctx, session);
                }
            }
            let add = text(
                ui.painter(),
                pos2(bar.right() - BAR_INSET, y),
                Align2::RIGHT_CENTER,
                if pages { "New Page" } else { "Add Frame" },
                17.0,
                Weight::Regular,
                ink,
            );
            if tapped(ui, add, "add") {
                self.stop(session);
                self.options_open = false;
                if session.add_frame() == 0 {
                    said = Some(Said::NoRoom);
                }
            }
            if !pages {
                let at = pos2(add.left() - 23.0, y);
                let s = text(ui.painter(), at, Align2::RIGHT_CENTER, "Settings", 17.0, Weight::Regular, ink);
                settings_at = s.center().x - bar.left();
                if tapped(ui, s, "settings") {
                    self.settings_open = !self.settings_open;
                    self.options_open = false;
                    self.colours = None;
                }
            }
            let track = Rect::from_min_size(bar.min + vec2(12.0, 42.0), vec2(width - 24.0, 44.0));
            self.timeline(ui, session, &frames, &slots, current, track);
        });
        // Escape closes the onion skins' colours, else the popover (the hex keypad takes it first)
        if (self.settings_open || self.options_open)
            && !self.takes_keys()
            && crate::ui::escape(ctx)
            && self.colours.take().is_none()
        {
            self.settings_open = false;
            self.options_open = false;
        }
        if self.settings_open && !pages {
            let left = (settings_at - 145.0).clamp(0.0, width - 290.0);
            self.settings_popover(ctx, look, session, &settings, bar, left, settings_at - left, palettes, harmony);
        } else if self.options_open
            && let Some(index) = current
        {
            let first = slots.iter().position(|f| *f == index);
            let place = slot_of(&slots, index, self.held).zip(first).map_or(0, |(at, first)| at - first);
            self.options_popover(ctx, session, &settings, &frames, index, place, bar);
        }
        said
    }

    /// Playing goes on: the next frame shows when it is due.
    fn play_on(&mut self, ctx: &egui::Context, session: &CanvasSession, settings: &AnimationSettings) {
        let Some(p) = self.playing.as_mut() else { return };
        let Some(due) = p.due else { return };
        let now = Instant::now();
        if now < due {
            ctx.request_repaint_after(due - now);
            return;
        }
        let len = u32::try_from(p.order.len()).unwrap_or(0);
        match animation_next_step(settings.playback, len, p.at, p.step) {
            Some(next) => {
                p.at = next.at;
                p.step = next.step;
                p.due = Some(now + frame_time(settings));
                ctx.request_repaint_after(frame_time(settings));
            }
            // a single pass ends on its last frame, still shown
            None => p.due = None,
        }
        if let Some(frame) = self.playing_frame() {
            session.set_play_frame(u32::try_from(frame).ok());
        }
    }

    fn play(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        let Some(settings) = session.animation_settings() else { return };
        let order = session.animation_sequence();
        let Some(&first) = order.first() else { return };
        self.settings_open = false;
        self.options_open = false;
        self.colours = None;
        self.playing = Some(Playing { order, at: 0, step: 1, due: Some(Instant::now() + frame_time(&settings)) });
        session.set_play_frame(Some(first));
        ctx.request_repaint_after(frame_time(&settings));
    }

    /// The frames' pictures read again when the artwork changed (at most every so often).
    fn read_thumbs(&mut self, ctx: &egui::Context, session: &CanvasSession, frames: &[u64]) {
        let edits = session.edits();
        if let Some((e, n, at)) = self.thumbs_read {
            if e == edits && n == frames.len() {
                return;
            }
            if at.elapsed() < THUMBS_AGAIN {
                ctx.request_repaint_after(THUMBS_AGAIN - at.elapsed());
                return;
            }
        }
        self.thumbs.retain(|id, _| frames.contains(id));
        for &id in frames {
            match session.frame_thumbnail(id, FRAME_THUMB_SIDE) {
                Some(t) => {
                    let image =
                        egui::ColorImage::from_rgba_premultiplied([t.width as usize, t.height as usize], &t.rgba);
                    self.thumbs
                        .insert(id, ctx.load_texture(format!("frame {id}"), image, egui::TextureOptions::LINEAR));
                }
                None => _ = self.thumbs.remove(&id),
            }
        }
        self.thumbs_read = Some((edits, frames.len(), Instant::now()));
    }

    /// The frames in the rounded `track`, the current (or played) one in the middle with a blue
    /// line under it; each frame's card 40 tall and as wide as the canvas's shape makes it.
    /// Dragged, the track scrolls through the frames, each showing as it passes the middle; let
    /// go, it coasts and settles on a frame, which becomes the current one. A tap on a frame
    /// makes it current (on the current one, its options open).
    fn timeline(
        &mut self,
        ui: &Ui,
        session: &CanvasSession,
        frames: &[u64],
        slots: &[usize],
        current: Option<usize>,
        track: Rect,
    ) {
        let last = slots.len().saturating_sub(1) as f32;
        let slot = current.and_then(|c| slot_of(slots, c, self.held));
        let focus = self.playing.as_ref().map(|p| p.at as usize).or(slot).unwrap_or(0);
        let info = session.info();
        let aspect = if info.width > 0 && info.height > 0 {
            (info.width as f32 / info.height as f32).clamp(0.25, 2.0)
        } else {
            0.66
        };
        let frame_width = FRAME_HEIGHT * aspect + 1.0;
        let pitch = frame_width + 1.0;
        let t = &mut self.track;
        // played frames step; a frame chosen or added slides into the middle (a drag takes over)
        if !t.dragging && t.focus != Some(focus) {
            if self.playing.is_some() || t.focus.is_none() {
                t.at = focus as f32;
                t.slide = None;
            } else if t.slide.is_none_or(|(_, to, _, _)| to != focus as f32) {
                t.slide = Some((t.at, focus as f32, Instant::now(), false));
            }
            t.focus = Some(focus);
        }
        let response = ui.interact(track, ui.scope_id().with("timeline"), Sense::click_and_drag());
        let scrub = |t: &mut Track, to: f32| {
            t.at = to.clamp(0.0, last);
            let i = t.at.round() as usize;
            if t.shown != Some(i) {
                t.shown = Some(i);
                session.set_play_frame(slots.get(i).and_then(|f| u32::try_from(*f).ok()));
            }
        };
        let mut settled = None;
        if slots.len() > 1 && response.drag_started() {
            // playing stops and the frame's options close
            if self.playing.take().is_some() {
                session.set_play_frame(None);
            }
            self.options_open = false;
            t.dragging = true;
            t.slide = None;
            t.shown = Some(focus);
        }
        if t.dragging {
            if response.dragged() {
                let to = t.at - response.drag_delta().x / pitch;
                scrub(t, to);
            }
            if response.drag_stopped() {
                // coasting as far as its speed carries it, then settling on a frame
                let velocity = ui.input(|i| i.pointer.velocity().x);
                let target = (t.at - velocity / pitch / 4.2).round().clamp(0.0, last);
                t.slide = Some((t.at, target, Instant::now(), true));
                t.dragging = false;
            }
        }
        if let Some((from, to, since, ends_drag)) = t.slide {
            let k = (since.elapsed().as_secs_f32() / SLIDE).min(1.0);
            let at = from + (to - from) * cubic_bezier(0.2, 0.8, 0.5, 1.0, k);
            if ends_drag {
                scrub(t, at);
            } else {
                t.at = at;
            }
            if k >= 1.0 {
                t.slide = None;
                if ends_drag {
                    settled = Some(to as usize);
                }
            } else {
                ui.ctx().request_repaint();
            }
        }
        let painter = ui.painter_at(track);
        painter.rect_filled(track, 18, argb(TRACK_FILL));
        let mut tapped = None;
        for (i, frame) in slots.iter().enumerate() {
            // a frame's held places: grey cards after it
            let held = i > 0 && slots.get(i - 1) == Some(frame);
            let x = track.left() + track.width() / 2.0 - frame_width / 2.0 + pitch * (i as f32 - t.at);
            if x <= track.left() - frame_width || x >= track.right() {
                continue;
            }
            let card = Rect::from_min_size(pos2(x, track.top() + 2.0), vec2(frame_width, FRAME_HEIGHT));
            painter.rect_filled(card, 6, if held { argb(HELD_FRAME) } else { Color32::WHITE });
            if let Some(texture) = frames.get(*frame).and_then(|id| self.thumbs.get(id)).filter(|_| !held) {
                let [w, h] = texture.size().map(|v| v.max(1) as f32);
                let room = card.shrink(2.0);
                let k = (room.width() / w).min(room.height() / h);
                let picture = Rect::from_center_size(room.center(), vec2(w * k, h * k));
                painter.image(
                    texture.id(),
                    picture,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            painter.rect_stroke(card, 6, Stroke::new(1.0, argb(FRAME_EDGE)), egui::StrokeKind::Inside);
            if response.clicked() && response.interact_pointer_pos().is_some_and(|p| card.contains(p)) {
                tapped = Some(i);
            }
        }
        painter.rect_stroke(track, 18, Stroke::new(1.0, argb(TRACK_EDGE)), egui::StrokeKind::Inside);
        let line = Rect::from_min_size(pos2(track.center().x - 12.0, track.bottom() + 3.0), vec2(24.0, 3.0));
        ui.painter().rect_filled(line, 1.5, argb(pc::ACCENT));
        if let Some(i) = settled {
            // let go: the frame in the middle becomes the current one
            self.go_to(session, frames, slots, i, current);
            self.track.shown = None;
            session.set_play_frame(None);
        }
        if let Some(i) = tapped {
            if self.playing.take().is_some() {
                session.set_play_frame(None);
            }
            if Some(i) == slot {
                self.options_open = !self.options_open;
                self.settings_open = false;
            } else {
                self.options_open = false;
                self.go_to(session, frames, slots, i, current);
            }
        }
    }

    /// Place `slot` of the timeline comes to the middle, its frame the current one.
    fn go_to(&mut self, session: &CanvasSession, frames: &[u64], slots: &[usize], slot: usize, current: Option<usize>) {
        let Some(&frame) = slots.get(slot) else { return };
        self.held = (frame, slot - slots.iter().position(|f| *f == frame).unwrap_or(slot));
        if Some(frame) != current
            && let Some(&id) = frames.get(frame)
        {
            session.select_layer(id);
        }
    }

    /// The settings (or the onion skins' colours), standing on the bar over Settings.
    #[allow(clippy::too_many_arguments)]
    fn settings_popover(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        a: &AnimationSettings,
        bar: Rect,
        left: f32,
        arrow_x: f32,
        palettes: &Palettes,
        harmony: HarmonyKind,
    ) {
        let colours_page = self.colours.is_some();
        let p3 = procraft_core::colours_in_p3(session.profile());
        let content = if colours_page {
            34.0 + 7.0 + 26.0 + 11.0 + PICKER_BODY + compact_colours::TABS
        } else {
            11.0 + TITLE_LINE + 21.0 + 3.0 * 40.0 + 2.0 * 2.0 + 10.0 + slider_group(3) + 9.0 + 44.5 * 2.0 + 2.0
        };
        let height = 8.0 + content + 8.0 + panel::ARROW_HEIGHT;
        let rect = Rect::from_min_size(pos2(bar.left() + left, bar.top() - 2.0 - height), vec2(290.0, height));
        let mut changed = a.clone();
        let mut keypad = Rect::NOTHING;
        egui::Area::new(Id::unique("animation settings")).fixed_pos(rect.min).order(Order::Foreground).show(
            ctx,
            |ui| {
                ui.allocate_rect(rect, Sense::click_and_drag());
                down_popover(ui, rect, arrow_x);
                let (left, right) = (rect.left() + 8.0, rect.right() - 8.0);
                let mut y = rect.top() + 8.0;
                if let Some((after, picker)) = self.colours.as_mut() {
                    // the back button at the panel's very edge, "Colors" and Reset
                    let back = Rect::from_min_size(pos2(rect.left(), y), vec2(44.0, 34.0));
                    look.icon(ui, &icons::CHEVRON_LEFT, back.center(), 34.0, argb(SLIDE_VALUE));
                    let title = pos2(back.right(), y + 17.0);
                    text(ui.painter(), title, Align2::LEFT_CENTER, "Colors", 23.0, Weight::Bold, argb(PANEL_TITLE));
                    let reset = text(
                        ui.painter(),
                        pos2(right, y + 17.0),
                        Align2::RIGHT_CENTER,
                        "Reset",
                        17.0,
                        Weight::Regular,
                        argb(SLIDE_VALUE),
                    );
                    if ui.interact(back, ui.scope_id().with("back"), Sense::click()).clicked() {
                        self.colours = None;
                        return;
                    }
                    if ui.interact(reset.expand(6.0), ui.scope_id().with("reset"), Sense::click()).clicked() {
                        changed.before = vec![0.0; 3];
                        changed.after = vec![0.0; 3];
                        *picker = CompactColours::new(0, 0.0);
                    }
                    y += 34.0 + 7.0;
                    // the two swatches: before the current frame, then after; a tap picks which
                    for (i, v) in [&changed.before, &changed.after].into_iter().enumerate() {
                        let swatch = Rect::from_min_size(pos2(left + 13.0 + 49.0 * i as f32, y), vec2(44.0, 26.0));
                        ui.painter().rect_filled(swatch, 7, argb(SWATCH_RIM));
                        ui.painter().rect_filled(swatch.shrink(1.0), 6, crate::colours::srgb(unit_rgb(v)));
                        if ui.interact(swatch, ui.scope_id().with(("swatch", i)), Sense::click()).clicked()
                            && (i == 1) != *after
                        {
                            *after = i == 1;
                            *picker = CompactColours::new(to_rgb(v), 0.0);
                        }
                    }
                    y += 26.0 + 11.0;
                    let before = picker.hsv;
                    // the 260-wide picker shown 282 wide, over its tabs
                    let at = pos2(left - 4.0, y);
                    let k = PICKER_SHOWN / 260.0;
                    let layer = Id::unique("onion picker");
                    let mut hex = Rect::NOTHING;
                    ui.ctx().set_transform_layer(
                        LayerId::new(Order::Tooltip, layer),
                        TSTransform::from_translation(at.to_vec2()) * TSTransform::from_scaling(k),
                    );
                    egui::Area::new(layer).fixed_pos(Pos2::ZERO).order(Order::Tooltip).show(ui.ctx(), |ui| {
                        hex = picker.picker(ui, 0.0, 0.0, palettes, harmony, p3);
                    });
                    keypad = Rect::from_min_size(at + hex.min.to_vec2() * k, hex.size() * k);
                    let tabs = Rect::from_min_size(
                        pos2(left - 4.0, y + PICKER_BODY),
                        vec2(PICKER_SHOWN, compact_colours::TABS),
                    );
                    picker.tabs(ui, look, tabs);
                    if picker.hsv != before {
                        let rgb = rgb_unit(picker.rgb());
                        if *after {
                            changed.after = rgb;
                        } else {
                            changed.before = rgb;
                        }
                    }
                    return;
                }
                text(
                    ui.painter(),
                    pos2(left + 14.0, y + 11.0),
                    Align2::LEFT_TOP,
                    "Settings",
                    23.0,
                    Weight::Bold,
                    argb(PANEL_TITLE),
                );
                y += 11.0 + TITLE_LINE + 21.0;
                // the playback: Loop, Ping-Pong, One Shot, the chosen one blue
                let modes = [
                    (AnimationPlayback::Loop, "Loop"),
                    (AnimationPlayback::PingPong, "Ping-Pong"),
                    (AnimationPlayback::OneShot, "One Shot"),
                ];
                let group = Rect::from_min_max(pos2(left, y), pos2(right, y + 3.0 * 40.0 + 4.0));
                ui.painter().rect_filled(group, 18, argb(GROUP));
                for (i, (mode, label)) in modes.into_iter().enumerate() {
                    if i > 0 {
                        ui.painter().rect_filled(
                            Rect::from_min_max(pos2(left, y), pos2(right, y + 2.0)),
                            0,
                            argb(ROW_LINE),
                        );
                        y += 2.0;
                    }
                    let row = Rect::from_min_max(pos2(left, y), pos2(right, y + 40.0));
                    let chosen = a.playback == mode;
                    if chosen {
                        let corner = |at: bool| if at { 18 } else { 0 };
                        let corners = egui::CornerRadius {
                            nw: corner(i == 0),
                            ne: corner(i == 0),
                            sw: corner(i == 2),
                            se: corner(i == 2),
                        };
                        ui.painter().rect_filled(row, corners, argb(pc::ACCENT));
                        look.icon(
                            ui,
                            &icons::MENU_TICK,
                            pos2(row.right() - 16.0 - 11.0, row.center().y),
                            22.0,
                            Color32::WHITE,
                        );
                    }
                    let ink = if chosen { Color32::WHITE } else { argb(PANEL_TITLE) };
                    text(
                        ui.painter(),
                        pos2(row.left() + 16.0, row.center().y),
                        Align2::LEFT_CENTER,
                        label,
                        17.0,
                        Weight::Regular,
                        ink,
                    );
                    if ui.interact(row, ui.scope_id().with(("playback", i)), Sense::click()).clicked() {
                        changed.playback = mode;
                    }
                    y += 40.0;
                }
                y += 10.0;
                let sliders = Rect::from_min_max(pos2(left, y), pos2(right, y + slider_group(3)));
                ui.painter().rect_filled(sliders, 18, argb(GROUP));
                y += 8.0;
                let onion = a.onion_frames;
                let onion_shown = match onion {
                    0 => "None".to_owned(),
                    MAX_ONION => "Max".to_owned(),
                    n => n.to_string(),
                };
                let rows: [(&str, String, f32); 3] = [
                    ("Frames Per Second", a.fps.to_string(), (a.fps as f32 - 1.0) / 59.0),
                    ("Onion skin frames", onion_shown, onion as f32 / MAX_ONION as f32),
                    ("Onion skin opacity", format!("{} %", (a.onion_opacity * 100.0).round()), a.onion_opacity),
                ];
                for (i, (label, value, fraction)) in rows.into_iter().enumerate() {
                    let row = Rect::from_min_max(pos2(left, y), pos2(right, y + 55.0));
                    if let Some(v) = value_slider(ui, row, label, &value, fraction, ("animation", i)) {
                        match i {
                            0 => changed.fps = 1 + (v * 59.0).round() as u32,
                            1 => changed.onion_frames = (v * MAX_ONION as f32).round() as u32,
                            _ => changed.onion_opacity = v,
                        }
                    }
                    y += 55.0 + 11.5;
                }
                y += 9.0 - 11.5;
                let group = Rect::from_min_max(pos2(left, y), pos2(right, y + 44.5 * 2.0 + 2.0));
                ui.painter().rect_filled(group, 18, argb(GROUP));
                let blend = Rect::from_min_max(pos2(left, y), pos2(right, y + 44.5));
                if toggle_row(ui, blend, "Blend primary frame", a.blend_primary) {
                    changed.blend_primary = !a.blend_primary;
                }
                y += 44.5;
                ui.painter().rect_filled(Rect::from_min_max(pos2(left, y), pos2(right, y + 2.0)), 0, argb(ROW_LINE));
                y += 2.0;
                let row = Rect::from_min_max(pos2(left, y), pos2(right, y + 44.5));
                let at = pos2(row.left() + 16.0, row.center().y);
                text(
                    ui.painter(),
                    at,
                    Align2::LEFT_CENTER,
                    "Onion skin colors",
                    17.0,
                    Weight::Regular,
                    argb(PANEL_TITLE),
                );
                if ui.interact(row, ui.scope_id().with("onion colours"), Sense::click()).clicked() {
                    self.colours = Some((false, CompactColours::new(to_rgb(&a.before), 0.0)));
                }
            },
        );
        if let Some((_, picker)) = self.colours.as_mut() {
            picker.keypad(ctx, look, keypad);
        }
        if changed != *a {
            session.set_animation_settings(changed);
        }
    }

    /// The current frame's options, standing on the middle of the bar: how long it is held,
    /// Duplicate, Delete, and Foreground for the last frame and Background for the first (both
    /// for a lone frame). Pages: Duplicate, Delete, and Background for the first.
    fn options_popover(
        &mut self,
        ctx: &egui::Context,
        session: &CanvasSession,
        a: &AnimationSettings,
        frames: &[u64],
        index: usize,
        place: usize,
        bar: Rect,
    ) {
        let Some(&id) = frames.get(index) else { return };
        // (a held place has neither)
        let foreground = !a.pages && index + 1 == frames.len() && place == 0;
        let background = index == 0 && place == 0;
        let ends = match (foreground, background) {
            (true, true) => 10.0 + 44.0 * 2.0 + 2.0,
            (false, false) => 0.0,
            _ => 10.0 + 44.0,
        };
        let hold_rows = if a.pages { 0.0 } else { 4.0 + 55.0 + 10.0 };
        // a plugin's in-betweens: a frame drawn between this one and the next
        let inbetween = self.tweens && !a.pages && place == 0 && index + 1 < frames.len();
        let own: &[&str] = if inbetween { &["Duplicate", "Delete", "In-between"] } else { &["Duplicate", "Delete"] };
        // a plugin's actions for the frame, after the app's own
        let mut actions: Vec<&str> = own.to_vec();
        actions.extend(self.frame_actions.iter().map(|a| a.label.as_str()));
        let rows = actions.len() as f32;
        let content = 11.0 + TITLE_LINE + 15.0 + hold_rows + 44.0 * rows + 2.0 * (rows - 1.0) + ends;
        let height = 8.0 + content + 8.0 + panel::ARROW_HEIGHT;
        let rect = Rect::from_min_size(pos2(bar.center().x - 130.0, bar.top() - 2.0 - height), vec2(260.0, height));
        let mut changed = a.clone();
        egui::Area::new(Id::unique("frame options")).fixed_pos(rect.min).order(Order::Foreground).show(ctx, |ui| {
            ui.allocate_rect(rect, Sense::click_and_drag());
            down_popover(ui, rect, 130.0);
            let (left, right) = (rect.left() + 8.0, rect.right() - 8.0);
            let mut y = rect.top() + 8.0;
            let title = if a.pages { "Page options" } else { "Frame options" };
            text(
                ui.painter(),
                pos2(left + 14.0, y + 11.0),
                Align2::LEFT_TOP,
                title,
                23.0,
                Weight::Bold,
                argb(PANEL_TITLE),
            );
            y += 11.0 + TITLE_LINE + 15.0;
            if !a.pages {
                let hold = session.frame_hold(id);
                let group = Rect::from_min_max(pos2(left, y), pos2(right, y + 4.0 + 55.0));
                ui.painter().rect_filled(group, 18, argb(GROUP));
                let row = Rect::from_min_max(pos2(left, y + 4.0), pos2(right, y + 4.0 + 55.0));
                let shown = if hold == 0 { "None".to_owned() } else { hold.to_string() };
                if let Some(v) =
                    value_slider(ui, row, "Hold duration", &shown, hold as f32 / MAX_HOLD as f32, ("hold", 0))
                {
                    self.holding = true;
                    session.change_frame_hold(id, ((v * MAX_HOLD as f32).round() as u32).min(MAX_HOLD), false);
                } else if std::mem::take(&mut self.holding) {
                    // let go: the whole drag is one undo step
                    session.change_frame_hold(id, hold, true);
                }
                y += 4.0 + 55.0 + 10.0;
            }
            let group = Rect::from_min_max(pos2(left, y), pos2(right, y + 44.0 * rows + 2.0 * (rows - 1.0)));
            ui.painter().rect_filled(group, 18, argb(GROUP));
            for (i, &label) in actions.iter().enumerate() {
                let row = Rect::from_min_max(pos2(left, y), pos2(right, y + 44.0));
                let at = pos2(row.left() + 16.0, row.center().y);
                text(ui.painter(), at, Align2::LEFT_CENTER, label, 17.0, Weight::Regular, argb(PANEL_TITLE));
                if ui.interact(row, ui.scope_id().with(("frame", i)), Sense::click()).clicked() {
                    self.options_open = false;
                    // on a held place it splits there, or is held one frame less
                    match (i, place) {
                        // a plugin's, run with this frame's layer current
                        _ if i >= own.len() => {
                            let key = self.frame_actions.get(i - own.len()).map(|a| a.key.clone());
                            if key.is_some() && session.select_layer(id) {
                                self.run_action = key;
                            }
                        }
                        (2, _) => {
                            if session.add_inbetween(id).is_none() {
                                crate::files::say(
                                    "An in-between goes between two frames of one layer each, with something drawn.",
                                );
                            }
                        }
                        (0, 0) => _ = session.duplicate_layer(id),
                        (0, _) => _ = session.split_held_frame(id, place as u32),
                        (_, 0) => _ = session.delete_layer(id),
                        _ => _ = session.drop_held_frame(id),
                    }
                }
                y += 44.0;
                if i + 1 < actions.len() {
                    ui.painter().rect_filled(
                        Rect::from_min_max(pos2(left, y), pos2(right, y + 2.0)),
                        0,
                        argb(ROW_LINE),
                    );
                    y += 2.0;
                }
            }
            if foreground || background {
                y += 10.0;
                let group = Rect::from_min_max(pos2(left, y), pos2(right, rect.bottom() - 8.0 - panel::ARROW_HEIGHT));
                ui.painter().rect_filled(group, 18, argb(GROUP));
                if foreground {
                    let row = Rect::from_min_max(pos2(left, y), pos2(right, y + 44.0));
                    if toggle_row(ui, row, "Foreground", a.foreground) {
                        changed.foreground = !a.foreground;
                    }
                    y += 44.0;
                }
                if foreground && background {
                    ui.painter().rect_filled(
                        Rect::from_min_max(pos2(left, y), pos2(right, y + 2.0)),
                        0,
                        argb(ROW_LINE),
                    );
                    y += 2.0;
                }
                if background {
                    let row = Rect::from_min_max(pos2(left, y), pos2(right, y + 44.0));
                    if toggle_row(ui, row, "Background", a.background) {
                        changed.background = !a.background;
                    }
                }
            }
        });
        if changed != *a {
            session.set_animation_settings(changed);
        }
    }
}

/// How long a frame shows at the animation's frames a second.
fn frame_time(a: &AnimationSettings) -> Duration {
    Duration::from_millis(1000 / u64::from(a.fps.max(1)))
}

/// A slider group of `n` sliders: 8 above the first, 11.5 between.
fn slider_group(n: usize) -> f32 {
    8.0 + 55.0 * n as f32 + 11.5 * (n.saturating_sub(1)) as f32
}

/// A panel standing on the bar at `rect`, its arrow pointing down `arrow_x` from its left edge.
fn down_popover(ui: &Ui, rect: Rect, arrow_x: f32) {
    let h = panel::ARROW_HEIGHT;
    let body = Rect::from_min_max(rect.min, pos2(rect.right(), rect.bottom() - h));
    ui.painter().rect_filled(body, 24, argb(POPOVER_FILL));
    let tip = rect.left() + arrow_x;
    ui.painter().add(egui::Shape::convex_polygon(
        vec![pos2(tip - h, body.bottom() - 0.5), pos2(tip + h, body.bottom() - 0.5), pos2(tip, rect.bottom())],
        argb(POPOVER_FILL),
        Stroke::NONE,
    ));
}

/// A setting's name and value over a bar dragged or tapped along, in `row` (55 tall). Returns
/// where it was set (0..1), while held.
fn value_slider(ui: &Ui, row: Rect, label: &str, value: &str, fraction: f32, id: (&str, usize)) -> Option<f32> {
    let (left, right) = (row.left() + 16.0, row.right() - 16.0);
    text(ui.painter(), pos2(left, row.top() + 6.5), Align2::LEFT_TOP, label, 17.0, Weight::Regular, argb(SLIDE_LABEL));
    text(
        ui.painter(),
        pos2(right, row.top() + 6.5),
        Align2::RIGHT_TOP,
        value,
        17.0,
        Weight::Regular,
        argb(SLIDE_VALUE),
    );
    let bar = Rect::from_min_max(pos2(left, row.top() + 27.5), pos2(right, row.top() + 49.5));
    let response = ui.interact(bar, ui.scope_id().with(id), Sense::click_and_drag());
    let y = bar.center().y;
    let x = left + fraction.clamp(0.0, 1.0) * bar.width();
    ui.painter().rect_filled(Rect::from_min_max(pos2(left, y - 1.5), pos2(right, y + 1.5)), 0, argb(SLIDE_TRACK));
    ui.painter().rect_filled(Rect::from_min_max(pos2(left, y - 1.5), pos2(x, y + 1.5)), 0, argb(pc::ACCENT));
    let knob_x = (x - 8.0).clamp(left, right - 16.0);
    ui.painter().rect_filled(Rect::from_min_size(pos2(knob_x, y - 5.0), vec2(16.0, 10.0)), 5, Color32::WHITE);
    (response.is_pointer_button_down_on() || response.clicked())
        .then(|| response.interact_pointer_pos())
        .flatten()
        .map(|p| ((p.x - left) / bar.width()).clamp(0.0, 1.0))
}

/// A row with its switch; true when tapped.
fn toggle_row(ui: &Ui, row: Rect, label: &str, on: bool) -> bool {
    let at = pos2(row.left() + 16.0, row.center().y);
    text(ui.painter(), at, Align2::LEFT_CENTER, label, 17.0, Weight::Regular, argb(PANEL_TITLE));
    crate::transform::switch(ui, pos2(row.right() - 6.0 - 63.0, row.center().y - 14.0), on, label);
    ui.interact(row, ui.scope_id().with(("toggle", label)), Sense::click()).clicked()
}

/// An sRGB 0..1 triple as 0xRRGGBB, and back.
fn to_rgb(v: &[f32]) -> u32 {
    v.iter().take(3).fold(0, |acc, x| (acc << 8) | (x.clamp(0.0, 1.0) * 255.0).round() as u32)
}

fn rgb_unit(rgb: u32) -> Vec<f32> {
    [16, 8, 0].iter().map(|s| ((rgb >> s) & 0xFF) as f32 / 255.0).collect()
}

fn unit_rgb(v: &[f32]) -> [f32; 3] {
    let at = |i: usize| v.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);
    [at(0), at(1), at(2)]
}
