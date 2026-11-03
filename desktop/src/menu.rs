//! The menus that pop up from the panels, as the Android app's (ui/Motion.kt's MenuPresence and the
//! menus on it): frosted glass, rounded, opening from a point with a slight overshoot and closing
//! smaller as they fade. While one is open, a press anywhere else only closes it.

use std::time::Instant;

use egui::emath::TSTransform;
use egui::{Align2, Id, LayerId, Order, Pos2, Rect, Sense, Ui, pos2, vec2};

use crate::icons;
use crate::look::{Look, Themed, Weight, argb, cubic_bezier, text};

/// How long a menu takes to open or close, seconds.
const MENU_TIME: f32 = 0.3;
/// Opening, it grows from almost nothing; closing, it shrinks to this as it fades.
const START_SCALE: f32 = 0.001;
const END_SCALE: f32 = 0.8;
/// The glass over the canvas.
pub const GLASS: Themed = Themed(0xCF313135, 0xFAF0F0F0);
pub const INK: Themed = Themed(0xFFFFFFFF, 0xFF000000);
pub const DISABLED: Themed = Themed(0x66FFFFFF, 0x66000000);
pub const DESTRUCTIVE: u32 = 0xFFFF453A;
/// An option's row.
pub const OPTION_HEIGHT: f32 = 38.0;
/// The brush library's menus (and the layers title's): this wide, rounded so, nearly solid over a
/// panel, edged, their items with icons, the groups ruled apart.
pub const LIBRARY_WIDTH: f32 = 253.0;
pub const LIBRARY_CORNER: f32 = 16.0;
pub const OVER_PANEL: Themed = Themed(0xFA222222, 0xFAF0F0F0);
pub const EDGE: u32 = 0xFF5A5D5F;
pub const RULE: u32 = 0xFF3F4347;
pub const DANGER: Themed = Themed(0xFFE84F51, 0xFFD93A3D);
pub const ITEM_HEIGHT: f32 = 40.0;

/// How a menu looks: its fill, an edge round it, whether the canvas is frosted behind it, how
/// round it is, and whether it casts a shadow.
#[derive(Clone, Copy)]
pub struct Glass {
    pub fill: Themed,
    pub edge: Option<u32>,
    pub frost: bool,
    pub corner: f32,
    pub shadow: bool,
}

/// The glass over the canvas, as the layer options have it, with their shadow.
pub const FROSTED: Glass = Glass { fill: GLASS, edge: None, frost: true, corner: 24.0, shadow: true };
/// Over a panel: nearly solid, edged.
pub const OVER_A_PANEL: Glass =
    Glass { fill: OVER_PANEL, edge: Some(EDGE), frost: false, corner: LIBRARY_CORNER, shadow: false };

/// A menu's shadow, as measured under the layer options: black at 14%, a Gaussian blur of sigma 15
/// and 6 lower.
const SHADOW_ALPHA: f32 = 0.14;
const SHADOW_SIGMA: f32 = 15.0;
const SHADOW_DROP: f32 = 6.0;

/// The shadow a menu over `rect` (rounded by `corner`) casts, drawn round it only: none of it
/// under the menu, which may be see-through. Rings of vertices out from its outline, each as dark
/// as the blurred, lowered rectangle is there.
fn shadow(painter: &egui::Painter, rect: Rect, corner: f32) {
    // the standard normal's distribution, near enough (a logistic fit)
    let below = |x: f32| 1.0 / (1.0 + (1.702 * x).exp());
    let cast = rect.translate(vec2(0.0, SHADOW_DROP));
    let half = cast.size() / 2.0 - vec2(corner, corner);
    // how far a point is outside the cast rectangle's rounded outline (inside: negative)
    let outside = |p: Pos2| {
        let q = (p - cast.center()).abs() - half;
        q.max(vec2(0.0, 0.0)).length() + q.x.max(q.y).min(0.0) - corner
    };
    // the outline, each point with its outward direction: a quarter turn per corner
    const PER_CORNER: usize = 12;
    let inner = rect.shrink(corner);
    let mut outline = Vec::with_capacity(PER_CORNER * 4);
    for (k, centre) in
        [inner.right_bottom(), inner.left_bottom(), inner.left_top(), inner.right_top()].into_iter().enumerate()
    {
        for i in 0..PER_CORNER {
            let a = (k as f32 + i as f32 / (PER_CORNER - 1) as f32) * std::f32::consts::FRAC_PI_2;
            let out = vec2(a.cos(), a.sin());
            outline.push((centre + out * corner, out));
        }
    }
    const RINGS: usize = 10;
    let reach = 3.0 * SHADOW_SIGMA + SHADOW_DROP;
    let mut mesh = egui::Mesh::default();
    for r in 0..=RINGS {
        let t = reach * r as f32 / RINGS as f32;
        for &(p, out) in &outline {
            let at = p + out * t;
            let a = SHADOW_ALPHA * below(outside(at) / SHADOW_SIGMA);
            mesh.colored_vertex(at, egui::Color32::from_black_alpha((a * 255.0).round() as u8));
        }
    }
    let n = outline.len() as u32;
    for r in 0..RINGS as u32 {
        for i in 0..n {
            let (a, b) = (r * n + i, r * n + (i + 1) % n);
            mesh.add_triangle(a, b, a + n);
            mesh.add_triangle(b, b + n, a + n);
        }
    }
    painter.add(mesh);
}

/// A menu's coming and going.
#[derive(Default)]
pub struct Menu {
    /// Open or not, since when (none: never opened, so shut).
    open: bool,
    since: Option<Instant>,
}

impl Menu {
    pub fn open(&mut self) {
        if !self.open {
            self.open = true;
            self.since = Some(Instant::now());
        }
    }

    pub fn close(&mut self) {
        if self.open {
            self.open = false;
            self.since = Some(Instant::now());
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// How it shows now, opacity and scale; none once it has closed.
    fn presence(&self) -> Option<(f32, f32)> {
        let t = (self.since?.elapsed().as_secs_f32() / MENU_TIME).clamp(0.0, 1.0);
        if self.open {
            let k = cubic_bezier(0.26, 1.22, 0.39, 1.01, t);
            Some((k.clamp(0.0, 1.0), START_SCALE + (1.0 - START_SCALE) * k))
        } else if t < 1.0 {
            let k = cubic_bezier(0.2, 0.8, 0.5, 1.0, t);
            Some((1.0 - k, 1.0 + (END_SCALE - 1.0) * k))
        } else {
            None
        }
    }

    /// Draws the menu at `rect` (its size when open), growing from and shrinking to `pivot`: its
    /// `glass` (any frost's blur going into `backdrops`), then `content`. A press elsewhere closes
    /// it (and goes no further), as does Escape.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        id: &str,
        rect: Rect,
        pivot: Pos2,
        glass: Glass,
        backdrops: &mut Vec<f32>,
        content: impl FnOnce(&mut Ui),
    ) {
        let corner = glass.corner;
        let Some((alpha, scale)) = self.presence() else { return };
        if alpha < 1.0 || scale != 1.0 {
            ctx.request_repaint();
        }
        if self.open {
            // the rest of the window takes no presses while it is open
            let screen = ctx.content_rect();
            egui::Area::new(Id::unique((id, "scrim"))).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
                let shield = ui.allocate_rect(screen, Sense::click_and_drag());
                if crate::ui::escape(ui.ctx()) || shield.is_pointer_button_down_on() || shield.clicked() {
                    self.close();
                }
            });
        }
        let area = Id::unique((id, "menu"));
        let shown = Rect::from_min_max(pivot + (rect.min - pivot) * scale, pivot + (rect.max - pivot) * scale);
        if glass.frost {
            let ppp = ctx.pixels_per_point();
            let blur = [shown.left(), shown.top(), shown.right(), shown.bottom(), corner * scale];
            backdrops.extend(blur.map(|v| v * ppp));
            backdrops.push(alpha);
        }
        let transform = TSTransform::from_translation(pivot.to_vec2())
            * TSTransform::from_scaling(scale)
            * TSTransform::from_translation(-pivot.to_vec2());
        ctx.set_transform_layer(LayerId::new(Order::Tooltip, area), transform);
        egui::Area::new(area).fixed_pos(rect.min).order(Order::Tooltip).interactable(self.open).show(ctx, |ui| {
            ui.multiply_opacity(alpha);
            ui.allocate_rect(rect, Sense::click());
            if glass.shadow {
                shadow(ui.painter(), rect, corner);
            }
            ui.painter().rect_filled(rect, corner, argb(glass.fill));
            if let Some(edge) = glass.edge {
                ui.painter().rect_stroke(rect, corner, egui::Stroke::new(1.0, argb(edge)), egui::StrokeKind::Inside);
            }
            content(ui);
        });
    }
}

/// An option's row at `row`: its label (greyed when not `enabled`, red when `destructive`) and a
/// tick when `checked`. True when tapped.
#[allow(clippy::too_many_arguments)]
pub fn option(
    ui: &Ui,
    look: &mut Look,
    row: Rect,
    label: &str,
    checked: bool,
    enabled: bool,
    destructive: bool,
) -> bool {
    let ink = if !enabled {
        argb(DISABLED)
    } else if destructive {
        argb(DESTRUCTIVE)
    } else {
        argb(INK)
    };
    text(ui.painter(), pos2(row.left() + 19.0, row.center().y), Align2::LEFT_CENTER, label, 17.0, Weight::Regular, ink);
    if checked {
        look.icon(ui, &icons::TICK, pos2(row.right() - 16.0 - 9.0, row.center().y), 18.0, ink);
    }
    let response = ui.interact(row, ui.scope_id().with(("option", label)), Sense::click());
    enabled && response.clicked()
}

/// Options' rows down from `top`, `width` across from `left`, each [`OPTION_HEIGHT`].
pub fn option_rows(left: f32, top: f32, width: f32, n: usize) -> impl Iterator<Item = Rect> {
    (0..n).map(move |i| Rect::from_min_size(pos2(left, top + OPTION_HEIGHT * i as f32), vec2(width, OPTION_HEIGHT)))
}

/// What a library-style menu lists: an item (its icon, label and ink), or a rule between groups
/// (this far clear above and below it).
pub enum Entry<'a> {
    Item(&'a icons::Icon, &'a str, Themed),
    /// An item that opens more (a chevron at its end).
    More(&'a icons::Icon, &'a str, Themed),
    Rule(f32),
}

/// The usual rule between groups.
pub const DIVIDER: Entry<'static> = Entry::Rule(10.0);

/// How tall a menu of `entries` is, 7 above and below them.
pub fn entries_height(entries: &[Entry]) -> f32 {
    7.0 * 2.0
        + entries
            .iter()
            .map(|e| match e {
                Entry::Item(..) | Entry::More(..) => ITEM_HEIGHT,
                Entry::Rule(gap) => 1.0 + gap * 2.0,
            })
            .sum::<f32>()
}

/// Lays out and draws `entries` down from `rect`'s top (7 in); returns the index of the one tapped.
pub fn entries(ui: &Ui, look: &mut Look, rect: Rect, entries: &[Entry]) -> Option<usize> {
    let mut y = rect.top() + 7.0;
    let mut tapped = None;
    for (i, e) in entries.iter().enumerate() {
        match e {
            Entry::Item(icon, label, ink) | Entry::More(icon, label, ink) => {
                let row = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), ITEM_HEIGHT));
                // its icon (a 40 frame) centred 29.5 in, the label from 52
                let ink = argb(*ink);
                look.icon(ui, icon, pos2(row.left() + 29.5, row.center().y), 40.0, ink);
                text(
                    ui.painter(),
                    pos2(row.left() + 52.0, row.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    18.0,
                    Weight::Regular,
                    ink,
                );
                if matches!(e, Entry::More(..)) {
                    look.icon(ui, &icons::MENU_CHEVRON, pos2(row.right() - 7.5 - 20.0, row.center().y), 40.0, ink);
                }
                if ui.interact(row, ui.scope_id().with(("entry", i)), Sense::click()).clicked() {
                    tapped = Some(i);
                }
                y += ITEM_HEIGHT;
            }
            Entry::Rule(gap) => {
                let line = Rect::from_min_size(pos2(rect.left() + 15.5, y + gap), vec2(rect.width() - 31.0, 1.0));
                ui.painter().rect_filled(line, 0, argb(RULE));
                y += 1.0 + gap * 2.0;
            }
        }
    }
    tapped
}

/// Where entry `index` of `entries` starts, down from the menu's top.
pub fn entry_top(entries: &[Entry], index: usize) -> f32 {
    7.0 + entries
        .iter()
        .take(index)
        .map(|e| match e {
            Entry::Item(..) | Entry::More(..) => ITEM_HEIGHT,
            Entry::Rule(gap) => 1.0 + gap * 2.0,
        })
        .sum::<f32>()
}
