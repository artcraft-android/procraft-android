//! The brush library, as the Android app's (ui/BrushLibrary.kt): sets on the left, the shown set's
//! brushes with a sample stroke on the right. Tapping a set shows it; tapping a brush chooses it.

use std::collections::HashMap;
use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Align2, Color32, Rect, ScrollArea, Sense, TextureHandle, TextureOptions, Ui, UiBuilder, pos2, vec2};
use procraft_core::{BrushInfo, BrushSetInfo, CanvasSession, ToolKind};

use crate::icons;
use crate::look::{Look, Weight, argb, pc};
use crate::panel::{self, PANEL_GLYPH, PANEL_TITLE};

const LIBRARY_WIDTH: f32 = 440.0;
const SETS_WIDTH: f32 = 168.0;
const HEADER_HEIGHT: f32 = 52.0;
const SET_HEIGHT: f32 = 50.0;
const BRUSH_HEIGHT: f32 = 80.0;
const BRUSH_GAP: f32 = 8.0;
/// The library ends this far above the window's bottom edge.
const BOTTOM_MARGIN: f32 = 25.0;
const SET_COLUMN: u32 = 0x00000000;
const SET_CELL: u32 = 0x2E5B5B5B;
const BRUSH_COLUMN: u32 = 0x26000000;
const NAME_PILL: u32 = 0x80000000;
/// The sample stroke's picture: half its own canvas, the cell showing it across its width.
const PREVIEW: (u32, u32) = (530, 162);
const PREVIEWS_A_FRAME: usize = 3;

/// A brush's sample stroke, and whether it runs under the name (which then sits on a dark pill).
struct Preview {
    texture: TextureHandle,
    shaded: bool,
}

#[derive(Default)]
pub struct Library {
    /// The set shown, once one is tapped (else the current brush's).
    shown_set: Option<usize>,
    previews: HashMap<(u32, u32), Option<Preview>>,
}

impl Library {
    /// The tool changed: the library shows its brush's set again.
    pub fn tool_changed(&mut self) {
        self.shown_set = None;
    }

    /// Shows the library hanging from `button`; returns where it is (points).
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        tool: ToolKind,
        button: Rect,
        backdrops: &mut Vec<f32>,
    ) -> Rect {
        let screen = ctx.content_rect();
        let (left, arrow_x) =
            panel::place(LIBRARY_WIDTH, button.center().x, screen, pc::SIDE_BAR_WIDTH + panel::POPOVER_GAP);
        let top = screen.top() + pc::BAR_HEIGHT;
        let rect = Rect::from_min_size(
            pos2(left, top),
            vec2(LIBRARY_WIDTH, (screen.bottom() - BOTTOM_MARGIN - top).max(200.0)),
        );
        let library = session.brush_library();
        let current = session.current_brush();
        let shown = self.shown_set.unwrap_or(current.set as usize).min(library.len().saturating_sub(1));
        egui::Area::new(egui::Id::unique("brush library")).fixed_pos(rect.min).order(egui::Order::Foreground).show(
            ctx,
            |ui| {
                // nothing under the panel takes a touch
                ui.allocate_rect(rect, Sense::click());
                panel::frame(ui, look, "library frame", rect, arrow_x, backdrops);
                let body = panel::body(rect);
                let title = match tool {
                    ToolKind::Erase => "Erase library",
                    ToolKind::Smudge => "Smudge library",
                    ToolKind::Paint => "Brush library",
                };
                panel::header(ui, look, body, title, 20.0, true, false);
                // the plus: new brushes and sets, and importing (its menu comes later)
                let plus = Rect::from_min_size(pos2(body.right() - 12.0 - 41.0, body.top() + 12.0), vec2(41.0, 40.0));
                look.icon(ui, &icons::PLUS_MEDIUM, plus.center(), 41.0, argb(PANEL_GLYPH));

                let lists = Rect::from_min_max(pos2(body.left(), body.top() + HEADER_HEIGHT), body.max);
                let sets = Rect::from_min_size(lists.min, vec2(SETS_WIDTH, lists.height()));
                let brushes = Rect::from_min_max(pos2(lists.left() + SETS_WIDTH, lists.top()), lists.max);
                if SET_COLUMN >> 24 != 0 {
                    ui.painter().rect_filled(sets, 0, argb(SET_COLUMN));
                }
                let corner = panel::POPOVER_CORNER;
                panel::rounded(ui, look, "library brushes", brushes, [0.0, 0.0, corner, 0.0], argb(BRUSH_COLUMN));

                ui.scope_builder(UiBuilder::new().max_rect(sets), |ui| {
                    ScrollArea::vertical()
                        .id_salt("library sets")
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .show(ui, |ui| {
                            ui.add_space(12.0);
                            for (i, set) in library.iter().enumerate() {
                                if set_row(ui, look, set, i == shown).clicked() && i != shown {
                                    self.shown_set = Some(i);
                                }
                                ui.add_space(12.0);
                            }
                        });
                });
                ui.scope_builder(UiBuilder::new().max_rect(brushes), |ui| {
                    ScrollArea::vertical()
                        .id_salt(("library brushes", shown))
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .show(ui, |ui| {
                            ui.add_space(9.75);
                            let Some(set) = library.get(shown) else { return };
                            let mut made = 0;
                            for b in &set.brushes {
                                // the current brush, or the brush last chosen in this set
                                let selected =
                                    (b.set, b.index) == (current.set, current.index) || set.chosen == Some(b.index);
                                if self.brush_row(ui, session, b, selected, &mut made).clicked() {
                                    session.select_brush(b.set, b.index);
                                }
                                ui.add_space(BRUSH_GAP);
                            }
                            ui.add_space(12.0 - BRUSH_GAP);
                        });
                });
            },
        );
        rect
    }

    /// A brush: its sample stroke filling the cell's height (its ends past the cell's sides),
    /// under its name.
    fn brush_row(
        &mut self,
        ui: &mut Ui,
        session: &CanvasSession,
        b: &BrushInfo,
        selected: bool,
        made: &mut usize,
    ) -> egui::Response {
        let row = ui.available_width();
        let (outer, response) = ui.allocate_exact_size(vec2(row, BRUSH_HEIGHT), Sense::click());
        let rect = outer.shrink2(vec2(12.0, 0.0));
        let painter = ui.painter_at(rect);
        if selected {
            painter.rect_filled(rect, 12, argb(pc::ACCENT));
        }
        let key = (b.set, b.index);
        if !self.previews.contains_key(&key) && ui.is_rect_visible(rect) {
            if *made < PREVIEWS_A_FRAME {
                *made += 1;
                let preview = session.brush_preview(b.set, b.index, PREVIEW.0, PREVIEW.1).map(|t| {
                    let shaded = under_name(&t.rgba, &b.name);
                    let image =
                        egui::ColorImage::from_rgba_premultiplied([t.width as usize, t.height as usize], &t.rgba);
                    Preview {
                        texture: ui.ctx().load_texture(format!("brush {key:?}"), image, TextureOptions::LINEAR),
                        shaded,
                    }
                });
                self.previews.insert(key, preview);
            } else {
                ui.ctx().request_repaint();
            }
        }
        let shaded = match self.previews.get(&key) {
            Some(Some(p)) => {
                let (pw, ph) = (PREVIEW.0 as f32, PREVIEW.1 as f32);
                let k = (rect.width() / pw).max(rect.height() / ph);
                let shown = Rect::from_center_size(rect.center(), vec2(pw * k, ph * k));
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                painter.image(p.texture.id(), shown, uv, Color32::WHITE);
                p.shaded
            }
            _ => false,
        };
        let ink = if selected { Color32::WHITE } else { argb(PANEL_TITLE) };
        let galley = one_line(ui, &b.name, 14.0, Weight::SemiBold, ink, rect.width() - 18.0 - 10.0);
        let at = rect.min + vec2(18.0, 11.75);
        if shaded {
            let pill = Rect::from_min_size(at, galley.size()).expand2(vec2(6.0, 2.0));
            painter.rect_filled(pill, pill.height() / 2.0, argb(NAME_PILL));
        }
        painter.galley(at, galley, ink);
        response
    }
}

/// A set: its icon and its name (two lines at most).
fn set_row(ui: &mut Ui, look: &mut Look, set: &BrushSetInfo, selected: bool) -> egui::Response {
    let width = ui.available_width();
    let (outer, response) = ui.allocate_exact_size(vec2(width, SET_HEIGHT), Sense::click());
    let rect = outer.shrink2(vec2(12.0, 0.0));
    ui.painter().rect_filled(rect, 12, panel::row_fill(selected, SET_CELL));
    let ink = if selected { Color32::WHITE } else { argb(PANEL_TITLE) };
    // a 40 frame centred 24 in
    let icon = icons::SET_ICONS.get(set.icon as usize).unwrap_or(&icons::BRUSH_SET);
    look.icon(ui, icon, pos2(rect.left() + 24.0, rect.center().y), 40.0, ink);
    let left = rect.left() + 4.0 + 40.0 + 1.0;
    let mut job = LayoutJob::default();
    job.append(&set.name, 0.0, format(14.0, Weight::SemiBold, ink));
    job.wrap = TextWrapping {
        max_width: rect.right() - 8.0 - left,
        max_rows: 2,
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    let galley = ui.painter().layout_job(job);
    let at = Align2::LEFT_CENTER.anchor_size(pos2(left, rect.center().y), galley.size());
    ui.painter().galley(at.min, galley, ink);
    response
}

fn format(size: f32, weight: Weight, colour: Color32) -> TextFormat {
    TextFormat { font_id: weight.font(size), color: colour, extra_letter_spacing: -0.045 * size, ..Default::default() }
}

/// Text on one line, cut short with an ellipsis past `max_width`.
fn one_line(ui: &Ui, text: &str, size: f32, weight: Weight, colour: Color32, max_width: f32) -> Arc<egui::Galley> {
    let mut job = LayoutJob::default();
    job.append(text, 0.0, format(size, weight, colour));
    job.wrap = TextWrapping { max_width, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    ui.painter().layout_job(job)
}

/// Whether a sample stroke (`PREVIEW` big, premultiplied RGBA) runs under its name: the name's box
/// (19.5 in, 14 to 26 down, about 7.7 a letter wide, in a cell the picture fills 80 high) covered
/// more than 2/255 on average.
fn under_name(rgba: &[u8], name: &str) -> bool {
    let (w, h) = (PREVIEW.0 as usize, PREVIEW.1 as usize);
    let k = PREVIEW.1 as f32 / BRUSH_HEIGHT;
    let x0 = ((19.5 + 6.85) * k) as usize;
    let x1 = (((19.5 + 6.85 + 7.7 * name.chars().count() as f32) * k) as usize).min(w);
    let (y0, y1) = (((14.0 * k) as usize).min(h), ((26.0 * k) as usize).min(h));
    if x1 <= x0 || y1 <= y0 {
        return false;
    }
    let mut cover = 0u64;
    for y in y0..y1 {
        let line = rgba.get((y * w + x0) * 4..(y * w + x1) * 4).unwrap_or(&[]);
        cover += line.as_chunks::<4>().0.iter().map(|&[_, _, _, a]| u64::from(a)).sum::<u64>();
    }
    cover > 2 * ((x1 - x0) * (y1 - y0)) as u64
}
