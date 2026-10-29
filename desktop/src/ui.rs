//! The interface over the canvas: the bars (see [`crate::chrome`]), the popovers hanging from
//! the top bar, and the keyboard's shortcuts.

use std::collections::HashMap;
use std::path::PathBuf;

use egui::{
    Align, Align2, Color32, Frame, Id, Key, KeyboardShortcut, Layout, Margin, Modifiers, Order, Rect, RichText,
    ScrollArea, Sense, Slider, Stroke, TextureHandle, TextureOptions, Ui, pos2, vec2,
};
use procraft_core::{BrushInfo, CanvasInfo, CanvasSession, LayerInfo, LayerType, ThumbnailImage, ToolKind};

use crate::chrome::{BarButton, Lit, SideBar};
use crate::files::Format;
use crate::input::Input;
use crate::look::{Look, argb, pc};

const POPOVER_FILL: Color32 = Color32::from_rgb(0x28, 0x2a, 0x2d);
/// A brush's preview (pixels), and how many are drawn a frame.
const PREVIEW: (u32, u32) = (360, 72);
const PREVIEWS_A_FRAME: usize = 3;
/// A layer's thumbnail (points), and how many are read a frame.
const THUMBNAIL: f32 = 40.0;
const THUMBNAILS_A_FRAME: usize = 4;
/// The brush library's lists, points.
const LIBRARY_HEIGHT: f32 = 440.0;

/// What the interface asks the app to do once the frame is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    New,
    Open,
    Save,
    SaveAs,
    Export(Format),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Popover {
    Actions,
    Brushes,
    Layers,
    Colour,
}

pub struct Interface {
    pub actions: Vec<Action>,
    /// Where the canvas shows, clear of the bar (points).
    pub canvas_rect: Rect,
    /// A popover is open: a press on the canvas only closes it.
    pub popover_open: bool,
    /// The file the artwork was last saved to (or opened from, if it saves back there).
    pub path: Option<PathBuf>,
    /// The artwork's edits when it was last saved: anything else is unsaved.
    pub saved_edits: u64,
    open: Option<Popover>,
    /// The colour as hue, saturation and brightness (0..1, of its sRGB values), and its hex
    /// code while typed.
    color: [f32; 3],
    hex: Option<String>,
    /// The set shown in the brush library.
    brush_set: Option<usize>,
    previews: HashMap<(u32, u32), Option<TextureHandle>>,
    /// Each layer's thumbnail and the version it shows.
    thumbnails: HashMap<u64, (u64, TextureHandle)>,
    /// The layer being renamed, and its name so far.
    renaming: Option<(u64, String)>,
    look: Look,
    side_bar: SideBar,
    styled: bool,
}

impl Default for Interface {
    fn default() -> Interface {
        Interface {
            actions: Vec::new(),
            canvas_rect: Rect::EVERYTHING,
            popover_open: false,
            path: None,
            saved_edits: 0,
            open: None,
            color: [0.0, 0.0, 0.0],
            hex: None,
            brush_set: None,
            previews: HashMap::new(),
            thumbnails: HashMap::new(),
            renaming: None,
            look: Look::default(),
            side_bar: SideBar::default(),
            styled: false,
        }
    }
}

impl Interface {
    /// `session` is shown now (at `scale` window pixels a point): it paints in the colour chosen.
    pub fn opened(&mut self, session: &CanvasSession, scale: f32) {
        self.thumbnails.clear();
        self.renaming = None;
        let [r, g, b] = hsv_to_rgb(self.color);
        session.set_color(r, g, b);
        self.place_canvas(session, scale);
    }

    /// Fitting the canvas keeps it clear of the bar.
    pub fn place_canvas(&self, session: &CanvasSession, scale: f32) {
        session.set_fit_insets(0.0, pc::BAR_HEIGHT * scale, 0.0, 0.0);
    }

    pub fn show(&mut self, ui: &mut Ui, session: &CanvasSession, input: &Input) {
        let ctx = ui.ctx().clone();
        if !self.styled {
            style(&ctx);
            self.styled = true;
        }
        let info = session.info();
        self.shortcuts(&ctx, session, &info);
        let lit = Lit {
            actions: self.open == Some(Popover::Actions),
            layers: self.open == Some(Popover::Layers),
            panel_open: matches!(self.open, Some(Popover::Layers | Popover::Colour)),
            ..Lit::default()
        };
        let colour = srgb(hsv_to_rgb(self.color));
        let bar = crate::chrome::top_bar(ui, &mut self.look, info.tool, colour, self.color[2], &lit);
        match bar.pressed {
            Some(BarButton::Gallery) => self.actions.push(Action::Open),
            Some(BarButton::Actions) => self.toggle(Popover::Actions),
            Some(BarButton::Layers) => self.toggle(Popover::Layers),
            Some(BarButton::Colour) => self.toggle(Popover::Colour),
            Some(b @ (BarButton::Paint | BarButton::Smudge | BarButton::Erase)) => {
                let tool = match b {
                    BarButton::Smudge => ToolKind::Smudge,
                    BarButton::Erase => ToolKind::Erase,
                    _ => ToolKind::Paint,
                };
                // the chosen tool again opens its brushes
                if info.tool == tool && self.open.is_none_or(|o| o == Popover::Brushes) {
                    self.toggle(Popover::Brushes);
                } else {
                    session.set_tool(tool);
                    self.brush_set = None;
                    self.open = None;
                }
            }
            Some(BarButton::Adjustments | BarButton::Selection | BarButton::Transform) | None => {}
        }
        self.canvas_rect = ui.available_rect_before_wrap();
        let mut backdrops = Vec::new();
        self.side_bar.show(ui, &mut self.look, session, &info, &mut backdrops);
        session.set_backdrops(backdrops);
        let tool_button = match info.tool {
            ToolKind::Smudge => BarButton::Smudge,
            ToolKind::Erase => BarButton::Erase,
            ToolKind::Paint => BarButton::Paint,
        };
        let anchor = |p: Popover| match p {
            Popover::Actions => bar.rect(BarButton::Actions),
            Popover::Brushes => bar.rect(tool_button),
            Popover::Layers => bar.rect(BarButton::Layers),
            Popover::Colour => bar.rect(BarButton::Colour),
        };
        if let Some(open) = self.open {
            self.popover(&ctx, session, open, anchor(open));
        }
        if input.moving_view() && !ctx.is_pointer_over_egui() {
            ctx.set_cursor_icon(egui::CursorIcon::Grab);
        }
        self.popover_open = self.open.is_some();
    }

    fn toggle(&mut self, popover: Popover) {
        self.open = if self.open == Some(popover) { None } else { Some(popover) };
    }

    /// The open popover, hanging from its button; a press anywhere else, or Escape, closes it.
    fn popover(&mut self, ctx: &egui::Context, session: &CanvasSession, which: Popover, button: Option<Rect>) {
        let width = match which {
            Popover::Actions => 260.0,
            Popover::Brushes => 560.0,
            Popover::Layers => 340.0,
            Popover::Colour => 300.0,
        };
        let Some(button) = button else {
            self.open = None;
            return;
        };
        let window = ctx.content_rect();
        let x = (button.right() - width).min(window.right() - width - 8.0).max(window.left() + 8.0);
        let shown = egui::Area::new(Id::unique(("popover", which)))
            .order(Order::Foreground)
            .fixed_pos(pos2(x, button.bottom() + 6.0))
            .show(ctx, |ui| {
                Frame::popup(ui.style()).fill(POPOVER_FILL).corner_radius(13).inner_margin(Margin::same(12)).show(
                    ui,
                    |ui| {
                        ui.set_width(width - 24.0);
                        match which {
                            Popover::Actions => self.files(ui),
                            Popover::Brushes => self.brushes(ui, session),
                            Popover::Layers => self.layers(ui, session),
                            Popover::Colour => self.colour(ui, session),
                        }
                    },
                );
            })
            .response;
        let pressed_outside = ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer.interact_pos().is_some_and(|p| !shown.rect.contains(p) && !button.contains(p))
        });
        if pressed_outside || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.open = None;
        }
    }

    /// For now, what the desktop does with files (the Actions panel comes later).
    fn files(&mut self, ui: &mut Ui) {
        let mut item = |ui: &mut Ui, text: &str, action: Action| {
            let row = egui::Button::new(text).min_size(vec2(ui.available_width(), 30.0)).frame(false);
            if ui.add(row).clicked() {
                self.actions.push(action);
                self.open = None;
            }
        };
        item(ui, "New Canvas", Action::New);
        item(ui, "Open…", Action::Open);
        ui.separator();
        item(ui, "Save", Action::Save);
        item(ui, "Save As…", Action::SaveAs);
        ui.separator();
        for format in Format::ALL {
            item(ui, &format!("Export {}", format.name()), Action::Export(format));
        }
    }

    fn brushes(&mut self, ui: &mut Ui, session: &CanvasSession) {
        let library = session.brush_library();
        let current = session.current_brush();
        let set = self.brush_set.unwrap_or(current.set as usize).min(library.len().saturating_sub(1));
        ui.label(RichText::new("Brush Library").strong());
        ui.add_space(4.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(170.0);
                ScrollArea::vertical().id_salt("sets").max_height(LIBRARY_HEIGHT).show(ui, |ui| {
                    for (i, s) in library.iter().enumerate() {
                        let row =
                            egui::Button::selectable(i == set, &s.name).min_size(vec2(ui.available_width(), 30.0));
                        if ui.add(row).clicked() {
                            self.brush_set = Some(i);
                        }
                    }
                });
            });
            ui.separator();
            ui.vertical(|ui| {
                ScrollArea::vertical()
                    .id_salt("brushes")
                    .max_height(LIBRARY_HEIGHT)
                    .min_scrolled_height(LIBRARY_HEIGHT)
                    .show(ui, |ui| {
                        let Some(s) = library.get(set) else { return };
                        let mut made = 0;
                        for brush in &s.brushes {
                            let chosen = brush.set == current.set && brush.index == current.index;
                            if self.brush_row(ui, session, brush, chosen, &mut made).clicked() {
                                session.select_brush(brush.set, brush.index);
                            }
                        }
                    });
            });
        });
    }

    /// A brush in the library: its preview stroke and its name.
    fn brush_row(
        &mut self,
        ui: &mut Ui,
        session: &CanvasSession,
        brush: &BrushInfo,
        chosen: bool,
        made: &mut usize,
    ) -> egui::Response {
        let width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(vec2(width, 62.0), Sense::click());
        let painter = ui.painter_at(rect);
        let fill = if chosen {
            argb(pc::ACCENT)
        } else if response.hovered() {
            Color32::from_white_alpha(12)
        } else {
            Color32::TRANSPARENT
        };
        painter.rect_filled(rect, 8, fill);
        let key = (brush.set, brush.index);
        if !self.previews.contains_key(&key) && *made < PREVIEWS_A_FRAME && ui.is_rect_visible(rect) {
            *made += 1;
            let image = session.brush_preview(brush.set, brush.index, PREVIEW.0, PREVIEW.1);
            let texture = image.map(|i| texture(ui.ctx(), &format!("brush {key:?}"), &i));
            self.previews.insert(key, texture);
        }
        match self.previews.get(&key) {
            Some(Some(t)) => {
                let shown = Rect::from_min_size(rect.min + vec2(6.0, 2.0), vec2(rect.width() - 12.0, 40.0));
                painter.image(t.id(), shown, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            Some(None) => {}
            None => ui.ctx().request_repaint(),
        }
        painter.text(
            rect.left_bottom() + vec2(10.0, -6.0),
            Align2::LEFT_BOTTOM,
            &brush.name,
            egui::FontId::proportional(13.0),
            if chosen { Color32::WHITE } else { Color32::from_gray(0xdc) },
        );
        response
    }

    fn layers(&mut self, ui: &mut Ui, session: &CanvasSession) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Layers").strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button(" + ").on_hover_text("New layer").clicked() {
                    session.add_layer();
                }
            });
        });
        ui.add_space(4.0);
        let layers = session.layers();
        let versions: HashMap<u64, u64> = session.layer_versions().into_iter().map(|v| (v.id, v.version)).collect();
        let collapsed: Vec<u64> =
            layers.iter().filter(|l| l.kind == LayerType::Group && l.collapsed).map(|l| l.id).collect();
        let parents: HashMap<u64, Option<u64>> = layers.iter().map(|l| (l.id, l.parent)).collect();
        // a layer in a closed group is out of sight
        let hidden = |l: &LayerInfo| {
            let mut at = l.parent;
            for _ in 0..64 {
                let Some(p) = at else { return false };
                if collapsed.contains(&p) {
                    return true;
                }
                at = parents.get(&p).copied().flatten();
            }
            false
        };
        let mut read = 0;
        ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
            for layer in layers.iter().filter(|l| !hidden(l)) {
                self.layer_row(ui, session, layer, versions.get(&layer.id).copied(), &mut read);
            }
            background_row(ui, session);
        });
        if let Some(current) = layers.iter().find(|l| l.current) {
            ui.separator();
            let mut opacity = current.opacity;
            let slider = ui.add(Slider::new(&mut opacity, 0.0..=1.0).text("Opacity").show_value(false));
            if slider.changed() || slider.drag_stopped() {
                // one step for the whole drag
                session.set_layer_opacity(current.id, opacity, !slider.dragged());
            }
            ui.horizontal(|ui| {
                if ui.button("Duplicate").clicked() {
                    session.duplicate_layer(current.id);
                }
                if ui.add_enabled(current.can_merge, egui::Button::new("Merge Down")).clicked() {
                    session.merge_down(current.id);
                }
                if ui.button("Clear").clicked() {
                    session.clear_layer();
                }
                if ui.button("Delete").clicked() {
                    session.delete_layer(current.id);
                }
            });
        }
    }

    /// A layer: its thumbnail and name (a click makes it current, a double click renames it),
    /// and whether it shows.
    fn layer_row(
        &mut self,
        ui: &mut Ui,
        session: &CanvasSession,
        layer: &LayerInfo,
        version: Option<u64>,
        read: &mut usize,
    ) {
        let fill = if layer.current { argb(pc::ACCENT) } else { Color32::from_white_alpha(6) };
        Frame::NONE.fill(fill).corner_radius(8).inner_margin(Margin::symmetric(6, 4)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(layer.depth as f32 * 14.0);
                if layer.kind == LayerType::Group {
                    let arrow = if layer.collapsed { "▸" } else { "▾" };
                    if ui.small_button(arrow).clicked() {
                        session.set_group_collapsed(layer.id, !layer.collapsed);
                    }
                } else {
                    self.thumbnail(ui, session, layer.id, version, read);
                }
                let renaming = self.renaming.as_mut().filter(|(id, _)| *id == layer.id);
                if let Some((_, name)) = renaming {
                    let edit = ui.add(egui::TextEdit::singleline(name).desired_width(150.0));
                    edit.request_focus();
                    if edit.lost_focus() {
                        let kept = ui.input(|i| !i.key_pressed(Key::Escape));
                        if let Some((id, name)) = self.renaming.take().filter(|_| kept) {
                            session.rename_layer(id, name);
                        }
                    }
                } else {
                    let kind = match layer.kind {
                        LayerType::Mask => " (mask)",
                        _ => "",
                    };
                    let label = egui::Label::new(format!("{}{kind}", layer.name)).sense(Sense::click()).truncate();
                    let name = ui.add(label);
                    if name.double_clicked() {
                        self.renaming = Some((layer.id, layer.name.clone()));
                    } else if name.clicked() {
                        session.select_layer(layer.id);
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let mut visible = layer.visible;
                    if ui.checkbox(&mut visible, "").on_hover_text("Shown").changed() {
                        session.set_layer_visible(layer.id, visible);
                    }
                });
            });
        });
        ui.add_space(3.0);
    }

    /// A layer's thumbnail, read again when its pixels' version is new (a few a frame).
    fn thumbnail(&mut self, ui: &mut Ui, session: &CanvasSession, id: u64, version: Option<u64>, read: &mut usize) {
        let (rect, response) = ui.allocate_exact_size(vec2(THUMBNAIL, THUMBNAIL), Sense::click());
        if response.clicked() {
            session.select_layer(id);
        }
        let stale = version.is_some_and(|v| self.thumbnails.get(&id).is_none_or(|(at, _)| *at != v));
        if stale {
            if *read < THUMBNAILS_A_FRAME {
                *read += 1;
                let side = (THUMBNAIL * ui.ctx().pixels_per_point()).round() as u32;
                if let (Some(image), Some(v)) = (session.layer_thumbnail(id, side), version) {
                    self.thumbnails.insert(id, (v, texture(ui.ctx(), &format!("layer {id}"), &image)));
                }
            } else {
                ui.ctx().request_repaint();
            }
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4, Color32::from_gray(0x50));
        if let Some((_, t)) = self.thumbnails.get(&id) {
            let size = t.size_vec2();
            let fit = (rect.width() / size.x.max(1.0)).min(rect.height() / size.y.max(1.0));
            let shown = Rect::from_center_size(rect.center(), size * fit);
            painter.rect_filled(shown, 0, Color32::WHITE);
            painter.image(t.id(), shown, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
    }

    /// The colour: a square of saturation (across) and brightness (up) for its hue, a strip of
    /// hues, and its hex code.
    fn colour(&mut self, ui: &mut Ui, session: &CanvasSession) {
        ui.label(RichText::new("Colours").strong());
        ui.add_space(6.0);
        let before = self.color;
        let [h, s, v] = &mut self.color;
        let width = ui.available_width();
        let (square, response) = ui.allocate_exact_size(vec2(width, width * 0.72), Sense::click_and_drag());
        if let Some(p) = response.interact_pointer_pos() {
            *s = ((p.x - square.left()) / square.width()).clamp(0.0, 1.0);
            *v = (1.0 - (p.y - square.top()) / square.height()).clamp(0.0, 1.0);
        }
        let hue = *h;
        paint_gradient(ui, square, |x, y| hsv_to_rgb([hue, x, 1.0 - y]));
        let at = pos2(square.left() + *s * square.width(), square.top() + (1.0 - *v) * square.height());
        ui.painter().circle_stroke(at, 7.0, Stroke::new(2.5, Color32::WHITE));
        ui.add_space(8.0);
        let (strip, response) = ui.allocate_exact_size(vec2(width, 22.0), Sense::click_and_drag());
        if let Some(p) = response.interact_pointer_pos() {
            *h = ((p.x - strip.left()) / strip.width()).clamp(0.0, 1.0);
        }
        paint_gradient(ui, strip, |x, _| hsv_to_rgb([x, 1.0, 1.0]));
        let x = strip.left() + *h * strip.width();
        let marker = Rect::from_center_size(pos2(x, strip.center().y), vec2(6.0, strip.height() + 4.0));
        ui.painter().rect_stroke(marker, 3, Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Middle);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Hex");
            let shown = hex_of(hsv_to_rgb(self.color));
            let text = self.hex.get_or_insert_with(|| shown.clone());
            let edit = ui.add(egui::TextEdit::singleline(text).desired_width(90.0).char_limit(7));
            if edit.lost_focus() {
                if let Some(rgb) = rgb_of_hex(text) {
                    // a grey keeps the hue it had
                    let [h, s, v] = rgb_to_hsv(rgb);
                    self.color = [if s > 0.0 { h } else { self.color[0] }, s, v];
                }
                self.hex = None;
            } else if !edit.has_focus() && *text != shown {
                self.hex = None;
            }
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
            ui.painter().rect_filled(rect, 6, srgb(hsv_to_rgb(self.color)));
        });
        if self.color != before {
            let [r, g, b] = hsv_to_rgb(self.color);
            session.set_color(r, g, b);
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context, session: &CanvasSession, info: &CanvasInfo) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let pressed = |modifiers: Modifiers, key: Key| {
            ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(modifiers, key)))
        };
        let command_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        // with Shift first: the shortcut without it would take it too
        if pressed(command_shift, Key::Z) || pressed(Modifiers::COMMAND, Key::Y) {
            session.redo();
        } else if pressed(Modifiers::COMMAND, Key::Z) {
            session.undo();
        }
        if pressed(command_shift, Key::S) {
            self.actions.push(Action::SaveAs);
        } else if pressed(Modifiers::COMMAND, Key::S) {
            self.actions.push(Action::Save);
        }
        if pressed(Modifiers::COMMAND, Key::N) {
            self.actions.push(Action::New);
        }
        if pressed(Modifiers::COMMAND, Key::O) {
            self.actions.push(Action::Open);
        }
        if pressed(Modifiers::COMMAND, Key::Num0) {
            session.settle_to_fit();
        }
        if pressed(Modifiers::COMMAND, Key::Num1) {
            session.settle_to_actual_size();
        }
        let tool = [(Key::B, ToolKind::Paint), (Key::S, ToolKind::Smudge), (Key::E, ToolKind::Erase)]
            .into_iter()
            .find(|(key, _)| pressed(Modifiers::NONE, *key));
        if let Some((_, tool)) = tool {
            session.set_tool(tool);
        }
        if pressed(Modifiers::NONE, Key::OpenBracket) {
            session.set_brush_size((info.size - 0.05).max(0.0));
        }
        if pressed(Modifiers::NONE, Key::CloseBracket) {
            session.set_brush_size((info.size + 0.05).min(1.0));
        }
    }
}

/// The background: its colour and whether it shows.
fn background_row(ui: &mut Ui, session: &CanvasSession) {
    let background = session.background();
    Frame::NONE.fill(Color32::from_white_alpha(6)).corner_radius(8).inner_margin(Margin::symmetric(6, 8)).show(
        ui,
        |ui| {
            ui.horizontal(|ui| {
                let mut colour = [background.r, background.g, background.b];
                let edit = egui::color_picker::color_edit_button_rgb(ui, &mut colour);
                if edit.changed() {
                    let [r, g, b] = colour;
                    session.set_background_color(r, g, b, true);
                }
                ui.label("Background colour");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let mut visible = background.visible;
                    if ui.checkbox(&mut visible, "").changed() {
                        session.set_background_visible(visible);
                    }
                });
            });
        },
    );
}

/// A premultiplied image as a texture.
fn texture(ctx: &egui::Context, name: &str, image: &ThumbnailImage) -> TextureHandle {
    let (w, h) = (image.width as usize, image.height as usize);
    let pixels = image.rgba.get(..w * h * 4).unwrap_or(&[]);
    let colour = if w > 0 && h > 0 && pixels.len() == w * h * 4 {
        egui::ColorImage::from_rgba_premultiplied([w, h], pixels)
    } else {
        egui::ColorImage::filled([1, 1], Color32::TRANSPARENT)
    };
    ctx.load_texture(name, colour, TextureOptions::LINEAR)
}

fn style(ctx: &egui::Context) {
    crate::look::install_fonts(ctx);
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        s.visuals.selection.bg_fill = argb(pc::ACCENT);
        s.visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        s.visuals.hyperlink_color = argb(pc::ACCENT);
        s.visuals.window_fill = POPOVER_FILL;
        s.visuals.panel_fill = crate::look::argb(pc::BAR);
        s.visuals.slider_trailing_fill = true;
        s.spacing.button_padding = vec2(10.0, 5.0);
        s.spacing.item_spacing = vec2(8.0, 6.0);
    });
}

/// Fills `rect` with `colour_at` its point (0..1 across and down), smoothly between samples.
fn paint_gradient(ui: &Ui, rect: Rect, colour_at: impl Fn(f32, f32) -> [f32; 3]) {
    const STEPS: u32 = 24;
    let mut mesh = egui::Mesh::default();
    for j in 0..=STEPS {
        for i in 0..=STEPS {
            let (x, y) = (i as f32 / STEPS as f32, j as f32 / STEPS as f32);
            mesh.colored_vertex(rect.lerp_inside(vec2(x, y)), srgb(colour_at(x, y)));
        }
    }
    let row = STEPS + 1;
    for j in 0..STEPS {
        for i in 0..STEPS {
            let a = j * row + i;
            mesh.add_triangle(a, a + 1, a + row);
            mesh.add_triangle(a + 1, a + row + 1, a + row);
        }
    }
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// A colour's sRGB values (0..1) as a colour to paint with.
fn srgb([r, g, b]: [f32; 3]) -> Color32 {
    let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgb(byte(r), byte(g), byte(b))
}

/// Hue, saturation and brightness (0..1) as sRGB values.
fn hsv_to_rgb([h, s, v]: [f32; 3]) -> [f32; 3] {
    let h = (h.rem_euclid(1.0)) * 6.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [r + m, g + m, b + m]
}

/// sRGB values as hue, saturation and brightness (0..1).
fn rgb_to_hsv([r, g, b]: [f32; 3]) -> [f32; 3] {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d <= 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    [h / 6.0, if max > 0.0 { d / max } else { 0.0 }, max]
}

fn hex_of(rgb: [f32; 3]) -> String {
    let c = srgb(rgb);
    format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b())
}

/// `#RRGGBB` (the # optional) as sRGB values.
fn rgb_of_hex(text: &str) -> Option<[f32; 3]> {
    let digits = text.trim().trim_start_matches('#');
    if digits.len() != 6 || !digits.is_ascii() {
        return None;
    }
    let value = u32::from_str_radix(digits, 16).ok()?;
    Some([16, 8, 0].map(|shift| ((value >> shift) & 0xff) as f32 / 255.0))
}
