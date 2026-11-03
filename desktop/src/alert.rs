//! An alert over everything, as the Android app's (ui/AlertUi.kt): the window dims as a dark glass
//! panel fades in, settling from a little larger; in it a title (after an icon), a message, and
//! Cancel beside the action, red when it destroys (an extra choice, blue, can sit between them,
//! the panel widening for it). A tap beside the panel cancels.

use std::time::Instant;

use egui::emath::TSTransform;
use egui::{Align2, Color32, Id, LayerId, Order, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

use crate::icons::{Icon, Part};
use crate::look::{Look, Themed, Weight, argb, cubic_bezier, pc, text};

const BUTTON_WIDTH: f32 = 192.0;
/// As measured, the panel is this much wider each side than its message's widest line (or its
/// title's), its buttons sharing it; never narrower than they need, nor wider than three.
const TEXT_SIDE: f32 = 27.5;
const MOST_WIDTH: f32 = BUTTON_WIDTH * 3.0 + BUTTON_GAP * 2.0 + SIDE * 2.0;
const BUTTON_GAP: f32 = 12.0;
const BUTTON_HEIGHT: f32 = 48.0;
const SIDE: f32 = 15.0;
const FILL: Themed = Themed(0xF21E1E1E, 0xF2E3E3E3);
const EDGE: u32 = 0x24FFFFFF;
const INK: Themed = Themed(0xFFC5C5C5, 0xFF222222);
const CANCEL_FILL: Themed = Themed(0xFF2D2D2D, 0xFFD9D9D9);
const DESTROY_FILL: Themed = Themed(0xFFFE2D2E, 0xFFFD0100);
const DIM: f32 = 0.7;
const START_SCALE: f32 = 1.1;
/// A page with its corner folded (14 x 18, the corner solid), as the Android app draws it.
const PAGE: Icon = Icon {
    name: "page",
    width: 24.0,
    height: 24.0,
    parts: &[Part {
        d: "M7 3H13.25L19 8.75V19a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2zM7.5 4.75H12.5V10.75H17.25V18.5a0.75 0.75 0 0 1-0.75 0.75H7.5a0.75 0.75 0 0 1-0.75-0.75V5.5a0.75 0.75 0 0 1 0.75-0.75z",
        stroke: 0.0,
        fill: true,
        even_odd: true,
    }],
};

/// How well a PDF keeps its pictures, asked before one is made: Good, Better or Best (JPEG at 60,
/// 85 or exactly); a tap beside the panel makes none.
pub fn pdf_quality() -> Asking {
    Asking {
        title: "PDF quality".into(),
        message: String::new(),
        action: "Best".into(),
        icon: Some(&PAGE),
        destructive: false,
        extra: Some("Better".into()),
        first: Some("Good".into()),
        alone: false,
    }
}

/// The JPEG quality the PDF quality alert's `answer` picked, if it picked one.
pub fn pdf_jpeg(answer: Answer) -> Option<u8> {
    match answer {
        Answer::First => Some(60),
        Answer::Extra => Some(85),
        Answer::Action => Some(100),
        Answer::Cancel => None,
    }
}
/// Coming, it takes this long; going, this long.
const IN: f32 = 0.3;
const OUT: f32 = 0.35;

/// What an alert asks.
#[derive(Clone)]
pub struct Asking {
    pub title: String,
    pub message: String,
    pub action: String,
    pub icon: Option<&'static Icon>,
    pub destructive: bool,
    pub extra: Option<String>,
    /// The first button's label in place of Cancel: it answers [`Answer::First`], and only a tap
    /// beside the panel cancels.
    pub first: Option<String>,
    /// Only the action, across the panel: a notice, no Cancel.
    pub alone: bool,
}

/// How an alert was answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Cancel,
    First,
    Action,
    Extra,
}

#[derive(Default)]
pub struct Alert {
    asking: Option<Asking>,
    open: bool,
    since: Option<Instant>,
}

impl Alert {
    /// Asks `title` (after `icon`), saying `message`, with Cancel and a red `action`.
    pub fn open(&mut self, title: &str, message: &str, action: &str, icon: Option<&'static Icon>) {
        self.ask(Asking {
            title: title.into(),
            message: message.into(),
            action: action.into(),
            icon,
            destructive: true,
            extra: None,
            first: None,
            alone: false,
        });
    }

    /// Tells `title` (after `icon`), saying `message`, with OK alone.
    pub fn tell(&mut self, title: &str, message: &str, icon: Option<&'static Icon>) {
        self.ask(Asking {
            title: title.into(),
            message: message.into(),
            action: "OK".into(),
            icon,
            destructive: false,
            extra: None,
            first: None,
            alone: true,
        });
    }

    pub fn ask(&mut self, asking: Asking) {
        self.asking = Some(asking);
        self.open = true;
        self.since = Some(Instant::now());
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    fn close(&mut self) {
        self.open = false;
        self.since = Some(Instant::now());
    }

    /// How far it shows, 0 to 1.
    fn shown(&self) -> f32 {
        let Some(since) = self.since else { return 0.0 };
        let t = since.elapsed().as_secs_f32();
        if self.open {
            cubic_bezier(0.42, 0.0, 0.58, 1.0, t / IN)
        } else {
            1.0 - cubic_bezier(0.0, 0.0, 0.58, 1.0, t / OUT)
        }
    }

    /// Draws it while it shows; returns how it was answered, when it is.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look) -> Option<Answer> {
        let shown = self.shown();
        if shown <= 0.0 {
            return None;
        }
        let asking = self.asking.clone()?;
        if (self.open && shown < 1.0) || !self.open {
            ctx.request_repaint();
        }
        let screen = ctx.content_rect();
        let buttons = if asking.alone {
            1.0
        } else if asking.extra.is_some() {
            3.0
        } else {
            2.0
        };
        let measure = ctx.layer_painter(LayerId::background());
        let need = BUTTON_WIDTH * buttons + BUTTON_GAP * (buttons - 1.0) + SIDE * 2.0;
        let widest = |s: &str, size: f32, weight: Weight| {
            s.lines()
                .map(|l| measure.layout_no_wrap(l.to_owned(), weight.font(size), argb(INK)).size().x)
                .fold(0.0, f32::max)
        };
        let title = widest(&asking.title, 26.0, Weight::Bold) + if asking.icon.is_some() { 24.0 + 8.0 } else { 0.0 };
        let words = widest(&asking.message, 16.0, Weight::Regular).max(title);
        let width = (words + TEXT_SIDE * 2.0).clamp(need, MOST_WIDTH.max(need));
        let button_width = (width - SIDE * 2.0 - BUTTON_GAP * (buttons - 1.0)) / buttons;
        // the message wrapped inside the panel, its lines centred and 20 apart
        let inner = width - SIDE * 2.0;
        let wrapped =
            crate::look::wrapped_centred(&measure, &asking.message, 16.0, Weight::Regular, argb(INK), inner, 20.0);
        let message = if asking.message.is_empty() { 0.0 } else { wrapped.size().y };
        let height = 21.0 + 31.0 + 13.0 + message + 27.0 + BUTTON_HEIGHT + 16.0;
        let panel = Rect::from_center_size(screen.center(), vec2(width, height));
        let mut answer = None;
        let area = Id::unique("alert");
        let k = START_SCALE + (1.0 - START_SCALE) * shown;
        // the panel settles from larger, about its middle; the dimming under it, in the same layer,
        // still covers the window as it does (never smaller than it)
        let c = panel.center().to_vec2();
        let transform =
            TSTransform::from_translation(c) * TSTransform::from_scaling(k) * TSTransform::from_translation(-c);
        ctx.set_transform_layer(LayerId::new(Order::Debug, area), transform);
        egui::Area::new(area).fixed_pos(screen.min).order(Order::Debug).interactable(self.open).show(ctx, |ui| {
            // nothing under it is touched while it is up, and a tap beside the panel cancels
            let beside = ui.allocate_rect(screen, Sense::click_and_drag());
            ui.interact(panel, ui.scope_id().with("panel"), Sense::click_and_drag());
            if crate::ui::escape(ui.ctx()) || beside.clicked() {
                answer = Some(Answer::Cancel);
            }
            ui.painter().rect_filled(screen, 0, Color32::from_black_alpha((DIM * shown * 255.0) as u8));
            ui.multiply_opacity(shown);
            let painter = ui.painter();
            painter.rect_filled(panel, 24, argb(FILL));
            painter.rect_stroke(panel, 24, Stroke::new(1.0, argb(EDGE)), StrokeKind::Inside);
            let ink = argb(INK);
            // the title, after its icon, centred
            let title_y = panel.top() + 21.0 + 31.0 / 2.0;
            let words = painter.layout_no_wrap(asking.title.clone(), Weight::Bold.font(26.0), ink).size().x;
            let icon_w = if asking.icon.is_some() { 24.0 + 8.0 } else { 0.0 };
            let left = panel.center().x - (icon_w + words) / 2.0;
            if let Some(icon) = asking.icon {
                look.icon(ui, icon, pos2(left + 12.0, title_y), 24.0, argb(pc::ACCENT));
            }
            text(
                ui.painter(),
                pos2(left + icon_w, title_y),
                Align2::LEFT_CENTER,
                &asking.title,
                26.0,
                Weight::Bold,
                ink,
            );
            if !asking.message.is_empty() {
                ui.painter().galley(pos2(panel.center().x, panel.top() + 21.0 + 31.0 + 13.0), wrapped.clone(), ink);
            }
            let top = panel.bottom() - 16.0 - BUTTON_HEIGHT;
            let action_fill = if asking.destructive { DESTROY_FILL } else { pc::ACCENT.into() };
            let mut choices = match &asking.first {
                _ if asking.alone => vec![],
                Some(first) => vec![(first.as_str(), CANCEL_FILL, Answer::First)],
                None => vec![("Cancel", CANCEL_FILL, Answer::Cancel)],
            };
            if let Some(extra) = &asking.extra {
                choices.push((extra.as_str(), pc::ACCENT.into(), Answer::Extra));
            }
            choices.push((asking.action.as_str(), action_fill, Answer::Action));
            for (i, (label, fill, which)) in choices.into_iter().enumerate() {
                let left = panel.left() + SIDE + (button_width + BUTTON_GAP) * i as f32;
                let rect = Rect::from_min_size(pos2(left, top), vec2(button_width, BUTTON_HEIGHT));
                ui.painter().rect_filled(rect, BUTTON_HEIGHT / 2.0, argb(fill));
                text(ui.painter(), rect.center(), Align2::CENTER_CENTER, label, 17.0, Weight::Medium, Color32::WHITE);
                if ui.interact(rect, ui.scope_id().with(("button", i)), Sense::click()).clicked() {
                    answer = Some(which);
                }
            }
        });
        if !self.open {
            return None;
        }
        if answer.is_some() {
            self.close();
        }
        answer
    }
}
