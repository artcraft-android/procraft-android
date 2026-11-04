//! The brush studio, as the Android app's (ui/BrushStudio.kt): the brush's settings page by page
//! (the pages' card on the left in a wide window, a rail of icons when upright), and the drawing
//! pad, a canvas of its own painting with the brush as it is being edited. Cancel puts the brush
//! back as it was; Done keeps the edits.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{
    Align2, Color32, Id, Order, Pos2, Rect, Sense, Stroke, TextureHandle, TextureId, Ui, UiBuilder, pos2, vec2,
};
use procraft_core::{CanvasSession, PenSample, PointerKind, SourceKind, slider_extras};

use crate::icons::{self, Icon};
use crate::look::{Look, Themed, Weight, argb, pc, text};

/// The mouse pointer's id among the pen's pointers (`pen.rs`): a tablet pen moving it keeps its lean
/// there.
const MOUSE_POINTER: u64 = u64::MAX;

const TABS_FILL: Themed = Themed(0xFF272727, 0xFFEBEBEB);
const BACK: Themed = Themed(0xFF1F1F1F, 0xFFF3F3F3);
const LINE: Themed = Themed(0xFF303030, 0xFFCFCFCF);
const HEADER: Themed = Themed(0xFF9C9C9C, 0xFF818181);
const LABEL: Themed = Themed(0xFFEDEDED, 0xFF343434);
const DIM: Themed = Themed(0xFF8E8E8E, 0xFF9A9A9A);
const OFF: Themed = Themed(0xFF505050, 0xFFD5D5D5);
const CARD_EDGE: Themed = Themed(0xFF373737, 0xFFDADADA);
const CARD_TITLE: Themed = Themed(0xFFC1C1C1, 0xFF4A4A4A);
const PAGE_INK: Themed = Themed(0xFF797979, 0xFF3E3E3E);
const SWITCH_FIXED: u32 = 0xFF515153;
const SWITCH_FIXED_KNOB: u32 = 0xFF9B9B9B;
const PILL_OFF: Themed = Themed(0xFF222222, 0xFFEDEDED);
const VALUE_OFF: Themed = Themed(0xFF646464, 0xFFA0A0A0);
const KNOB_OFF: Themed = Themed(0xFF353535, 0xFFD0D0D0);
const LIST: Themed = Themed(0xFF242424, 0xFFFFFFFF);
const LIST_LINE: Themed = Themed(0xFF202020, 0xFFE5E5E5);
const SEGMENTS: Themed = Themed(0xFF3C3C3C, 0xFFDCDCDC);
const MATERIAL: Themed = Themed(0xFF232323, 0xFFECECEC);
const MATERIAL_INK: Themed = Themed(0xFFB1B1B1, 0xFF4A4A4A);
const PILL: Themed = Themed(0xFF292929, 0xFFE5E5E5);
const TRACK: Themed = Themed(0xFF333333, 0xFFE5E5E5);
const TAB_ICON: Themed = Themed(0xFFAAAAAA, 0xFF3D3D3D);
const BUTTON: Themed = Themed(0xFF1C1C1C, 0xFFFBFBFB);
const BUTTON_EDGE: Themed = Themed(0xFF383838, 0xFFE0E0E0);
const BUTTON_INK: Themed = Themed(0xFFAAAAAA, 0xFF585858);
const CARD: Themed = Themed(0xFF313131, 0xFFE5E5E5);
const PREVIEW_FILL: Themed = Themed(0xFF2A2A2A, 0xFFE8E8E8);
const PREVIEW_EDGE: Themed = Themed(0xFF4A4A4A, 0xFFD0D0D0);
const PREVIEW_NAME: Themed = Themed(0xFFC5C5C5, 0xFF3A3A3A);
const DONE: u32 = 0xFF0190FF;
const SETTINGS_WIDTH: f32 = 323.0;
const PAD_HEIGHT: f32 = 510.0;
const TABS_WIDTH: f32 = 81.0;
/// How far in from the track's ends a slider's knob stops.
const KNOB_INSET: f32 = 5.0;
/// The pad's colours to paint with (white the first time, as the pad is dark), and its own grey.
const PAD_COLOURS: [u32; 8] = [0xF83045, 0xFCBD03, 0x85D921, 0x2BB5E6, 0x4956A5, 0xDA3D92, 0xFFFFFF, 0x000000];
const PAD_WHITE: usize = 6;
const PAD_BLACK: usize = 7;
const PAD_BACK: f32 = 31.0 / 255.0;
const PAD_BACK_LIGHT: f32 = 243.0 / 255.0;
/// The sample stroke is drawn again this long after a control stops moving.
const PREVIEW_DELAY: Duration = Duration::from_millis(60);

/// How a slider reads: see `shown_value`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shown {
    Percent,
    Both,
    Count,
    Tip,
    Movement,
    Zoom,
    Charge,
    Size,
    Follow,
    Grade,
    Double,
    Twice,
    Metal,
    Gloss,
}

/// One control in the studio.
enum Control {
    /// A slider: its key, name, how it reads, and the choice another control must be on for it
    /// to be used (greyed otherwise).
    Slider(&'static str, &'static str, Shown, Option<(&'static str, i32)>),
    Toggle(&'static str, &'static str, Option<(&'static str, i32)>),
    Choice(&'static str, &'static [&'static str]),
    Kinds(&'static str, &'static [(&'static Icon, &'static str)]),
    /// The taper's two ends on a bar (start, end), with the switch linking them under it.
    TaperBar(&'static str, &'static str, &'static str),
    PressureCurve,
    TiltAngle,
    Roundness,
    Source(&'static str, SourceKind),
    /// A blend mode, picked from a list.
    Picker(&'static str, &'static str),
    /// A card that a tap on its title opens to show its controls.
    Disclosure(&'static str, &'static [Control]),
    PreviewCard,
    About,
}

struct Section {
    title: Option<&'static str>,
    controls: &'static [Control],
}

/// A page: its name, its icon (shown this wide) and its sections.
struct Page {
    name: &'static str,
    icon: &'static Icon,
    icon_size: f32,
    sections: &'static [Section],
}

const fn slider(key: &'static str, label: &'static str) -> Control {
    Control::Slider(key, label, Shown::Percent, None)
}

const fn shown(key: &'static str, label: &'static str, shown: Shown) -> Control {
    Control::Slider(key, label, shown, None)
}

const fn toggle(key: &'static str, label: &'static str) -> Control {
    Control::Toggle(key, label, None)
}

const FILTERING: &[&str] = &["No filtering", "Classic filtering", "Improved filtering"];

const PAGES: &[Page] = &[
    Page {
        name: "Stroke path",
        icon: &icons::STUDIO_STROKE,
        icon_size: 33.0,
        sections: &[Section {
            title: Some("Stroke properties"),
            controls: &[
                slider("spacing", "Spacing"),
                slider("spacing_jitter", "Spacing Jitter"),
                slider("jitter_lateral", "Jitter Lateral"),
                slider("jitter_linear", "Jitter Linear"),
                slider("falloff", "Fall off"),
            ],
        }],
    },
    Page {
        name: "Stabilization",
        icon: &icons::STUDIO_STABILIZE,
        icon_size: 45.0,
        sections: &[
            Section {
                title: Some("StreamLine"),
                controls: &[slider("streamline", "Amount"), slider("streamline_pressure", "Pressure")],
            },
            Section { title: Some("Stabilization"), controls: &[slider("stabilization", "Amount")] },
            Section {
                title: Some("Motion filtering"),
                controls: &[slider("motion_filtering", "Amount"), slider("motion_expression", "Expression")],
            },
        ],
    },
    Page {
        name: "Taper",
        icon: &icons::STUDIO_TAPER,
        icon_size: 41.0,
        sections: &[
            Section {
                title: Some("Pressure taper"),
                controls: &[
                    Control::TaperBar("pen_taper_start", "pen_taper_end", "pen_taper_link"),
                    slider("pen_taper_size", "Size"),
                    slider("pen_taper_opacity", "Opacity"),
                    slider("pen_taper_pressure", "Pressure"),
                    shown("pen_taper_tip", "Tip", Shown::Tip),
                    toggle("pen_tip_animation", "Tip animation"),
                ],
            },
            Section {
                title: Some("Touch taper"),
                controls: &[
                    Control::TaperBar("touch_taper_start", "touch_taper_end", "touch_taper_link"),
                    slider("touch_taper_size", "Size"),
                    slider("touch_taper_opacity", "Opacity"),
                    shown("touch_taper_tip", "Tip", Shown::Tip),
                ],
            },
            Section { title: Some("Taper properties"), controls: &[toggle("classic_taper", "Classic taper")] },
        ],
    },
    Page {
        name: "Shape",
        icon: &icons::STUDIO_SHAPE,
        icon_size: 45.0,
        sections: &[
            Section { title: None, controls: &[Control::Source("Shape Source", SourceKind::Shape)] },
            Section {
                title: Some("Input style"),
                controls: &[
                    Control::Choice("input_style", &["Touch only", "Azimuth", "Azimuth and barrel roll"]),
                    // only with the barrel roll to follow
                    Control::Toggle("relative_to_stroke", "Relative to stroke", Some(("input_style", 2))),
                ],
            },
            Section {
                title: Some("Touch properties"),
                controls: &[shown("shape_rotation", "Rotation", Shown::Follow)],
            },
            Section {
                title: Some("Shape properties"),
                controls: &[
                    shown("scatter", "Scatter", Shown::Twice),
                    shown("count", "Count", Shown::Count),
                    slider("count_jitter", "Count jitter"),
                    toggle("randomized", "Randomized"),
                    toggle("flip_x", "Flip X"),
                    toggle("flip_y", "Flip Y"),
                    Control::Roundness,
                    shown("pressure_roundness", "Pressure Roundness", Shown::Both),
                    shown("tilt_roundness", "Tilt roundness", Shown::Both),
                    slider("roundness_jitter_y", "Roundness Vertical Jitter"),
                    slider("roundness_jitter_x", "Roundness Horizontal Jitter"),
                ],
            },
            Section { title: Some("Shape filtering"), controls: &[Control::Choice("shape_filtering", FILTERING)] },
        ],
    },
    Page {
        name: "Grain",
        icon: &icons::STUDIO_GRAIN,
        icon_size: 45.0,
        sections: &[
            Section { title: None, controls: &[Control::Source("Grain Source", SourceKind::Grain)] },
            Section {
                title: Some("Grain behavior"),
                controls: &[
                    Control::Kinds(
                        "grain_behavior",
                        &[(&icons::GRAIN_MOVING, "Moving"), (&icons::GRAIN_TEXTURIZED, "Texturized")],
                    ),
                    shown("grain_movement", "Movement", Shown::Movement),
                    slider("grain_scale", "Scale"),
                    shown("grain_zoom", "Zoom", Shown::Zoom),
                    shown("grain_rotation", "Rotation", Shown::Follow),
                    slider("grain_depth", "Depth"),
                    slider("grain_depth_min", "Depth minimum"),
                    slider("grain_depth_jitter", "Depth jitter"),
                    toggle("grain_offset_jitter", "Offset jitter"),
                    Control::Picker("grain_blend", "Blend mode"),
                    shown("grain_brightness", "Brightness", Shown::Both),
                    shown("grain_contrast", "Contrast", Shown::Both),
                ],
            },
            Section { title: Some("Grain filtering"), controls: &[Control::Choice("grain_filtering", FILTERING)] },
            Section {
                title: Some("3D grain behaviour"),
                controls: &[toggle("grain_follows_camera", "Grain follows camera")],
            },
        ],
    },
    Page {
        name: "Rendering",
        icon: &icons::STUDIO_RENDERING,
        icon_size: 37.0,
        sections: &[
            Section {
                title: Some("Rendering mode"),
                controls: &[Control::Choice(
                    "rendering_mode",
                    &[
                        "Light Glaze",
                        "Uniformed Glaze",
                        "Intense Glaze",
                        "Heavy Glaze",
                        "Uniform Blending",
                        "Intense Blending",
                    ],
                )],
            },
            Section {
                title: Some("Blending"),
                controls: &[
                    slider("flow", "Flow"),
                    slider("wet_edges", "Wet edges"),
                    slider("burnt_edges", "Burnt edges"),
                    Control::Picker("burnt_blend", "Burnt edges mode"),
                    Control::Picker("blend_mode", "Blend mode"),
                    toggle("luminance_blending", "Luminance blending"),
                    toggle("alpha_threshold", "Alpha Threshold"),
                    Control::Slider(
                        "alpha_threshold_amount",
                        "Threshold Amount",
                        Shown::Percent,
                        Some(("alpha_threshold", 1)),
                    ),
                    // only for a brush paired with another, which none are yet
                    Control::Toggle("classic_combine", "Classic Normal Combine mode", Some(("dual_brush", 1))),
                ],
            },
        ],
    },
    Page {
        name: "Wet Mix",
        icon: &icons::STUDIO_WET,
        icon_size: 45.0,
        sections: &[Section {
            title: Some("Wet mix"),
            controls: &[
                slider("wet_dilution", "Dilution"),
                shown("wet_charge", "Charge", Shown::Charge),
                slider("wet_attack", "Attack"),
                slider("wet_pull", "Pull"),
                shown("wet_grade", "Grade", Shown::Grade),
                slider("wet_blur", "Blur"),
                slider("wet_blur_jitter", "Blur jitter"),
                slider("wet_jitter", "Wetness jitter"),
            ],
        }],
    },
    Page {
        name: "Color dynamics",
        icon: &icons::STUDIO_COLOR,
        icon_size: 41.0,
        sections: &[
            Section {
                title: Some("Stamp color jitter"),
                controls: &[
                    slider("stamp_hue", "Hue"),
                    slider("stamp_saturation", "Saturation"),
                    slider("stamp_lightness", "Lightness"),
                    slider("stamp_darkness", "Darkness"),
                    slider("stamp_secondary", "Secondary color"),
                ],
            },
            Section {
                title: Some("Stroke color jitter"),
                controls: &[
                    slider("stroke_hue", "Hue"),
                    slider("stroke_saturation", "Saturation"),
                    slider("stroke_lightness", "Lightness"),
                    slider("stroke_darkness", "Darkness"),
                    slider("stroke_secondary", "Secondary color"),
                ],
            },
            Section {
                title: Some("Color pressure"),
                controls: &[
                    shown("pressure_hue", "Hue", Shown::Both),
                    shown("pressure_saturation", "Saturation", Shown::Both),
                    shown("pressure_brightness", "Brightness", Shown::Both),
                    slider("pressure_secondary", "Secondary Color"),
                ],
            },
            Section {
                title: Some("Color tilt"),
                controls: &[
                    shown("tilt_hue", "Hue", Shown::Both),
                    shown("tilt_saturation", "Saturation", Shown::Both),
                    shown("tilt_brightness", "Brightness", Shown::Both),
                    slider("tilt_secondary", "Secondary Color"),
                ],
            },
            Section {
                title: Some("Color barrel roll"),
                controls: &[
                    slider("roll_hue", "Hue"),
                    slider("roll_saturation", "Saturation"),
                    slider("roll_brightness", "Brightness"),
                    slider("roll_secondary", "Secondary Color"),
                ],
            },
        ],
    },
    Page {
        name: "Dynamics",
        icon: &icons::STUDIO_DYNAMICS,
        icon_size: 37.0,
        sections: &[
            Section {
                title: Some("Speed"),
                controls: &[
                    shown("speed_size", "Size", Shown::Both),
                    shown("speed_opacity", "Opacity", Shown::Both),
                    slider("speed_spacing", "Spacing"),
                ],
            },
            Section {
                title: Some("Jitter"),
                controls: &[slider("jitter_size", "Size"), slider("jitter_opacity", "Opacity")],
            },
        ],
    },
    Page {
        name: "Stylus",
        icon: &icons::STUDIO_STYLUS,
        icon_size: 43.0,
        sections: &[
            Section {
                title: Some("Pressure"),
                controls: &[
                    Control::PressureCurve,
                    shown("pressure_size", "Size", Shown::Both),
                    slider("pressure_flow", "Opacity"),
                    shown("pressure_opacity", "Flow", Shown::Both),
                    slider("pressure_bleed", "Bleed"),
                ],
            },
            Section {
                title: Some("Tilt"),
                controls: &[
                    Control::TiltAngle,
                    slider("tilt_opacity", "Opacity"),
                    slider("tilt_gradation", "Gradation"),
                    slider("tilt_bleed", "Bleed"),
                    slider("tilt_size", "Size"),
                    toggle("tilt_size_compression", "Size compression"),
                ],
            },
            Section {
                title: Some("Barrel roll"),
                controls: &[
                    shown("roll_size", "Size", Shown::Both),
                    slider("roll_opacity", "Opacity"),
                    slider("roll_bleed", "Bleed"),
                ],
            },
            Section {
                title: Some("Cursor outline"),
                controls: &[Control::Choice("cursor_outline", &["None", "Contrast", "Active color"])],
            },
            Section { title: Some("Hover"), controls: &[slider("hover_pressure", "Estimated pressure")] },
            Section {
                title: Some("Hover fill"),
                controls: &[Control::Choice("hover_fill", &["None", "Shape", "All"])],
            },
        ],
    },
    Page {
        name: "Properties",
        icon: &icons::STUDIO_PROPERTIES,
        icon_size: 49.0,
        sections: &[
            Section {
                title: Some("Brush properties"),
                controls: &[toggle("orient_to_screen", "Orient to screen"), slider("smudge_pull", "Smudge Pull")],
            },
            Section {
                title: Some("Brush behavior"),
                controls: &[
                    shown("max_size", "Maximum size", Shown::Size),
                    shown("min_size", "Minimum size", Shown::Size),
                    slider("max_opacity", "Maximum opacity"),
                    slider("min_opacity", "Minimum opacity"),
                ],
            },
        ],
    },
    Page {
        name: "Materials",
        icon: &icons::STUDIO_MATERIALS,
        icon_size: 29.0,
        sections: &[Section {
            title: Some("3D Materials"),
            controls: &[
                Control::Disclosure(
                    "Metallic",
                    &[
                        shown("metallic_amount", "Amount", Shown::Metal),
                        Control::Source("Metallic Source", SourceKind::Metallic),
                        slider("metallic_scale", "Scale"),
                    ],
                ),
                Control::Disclosure(
                    "Roughness",
                    &[
                        shown("roughness_amount", "Amount", Shown::Gloss),
                        Control::Source("Roughness Source", SourceKind::Roughness),
                        slider("roughness_scale", "Scale"),
                    ],
                ),
            ],
        }],
    },
    Page {
        name: "Preview",
        icon: &icons::STUDIO_PREVIEW,
        icon_size: 45.0,
        sections: &[
            Section { title: Some("Preview render"), controls: &[Control::PreviewCard] },
            Section {
                title: Some("Preview properties"),
                controls: &[
                    toggle("preview_stamp", "Use stamp preview"),
                    slider("preview_size", "Size"),
                    slider("preview_pressure_min", "Pressure Minimum"),
                    shown("preview_pressure_scale", "Pressure Scale", Shown::Double),
                    toggle("preview_wet_mix", "Wet Mix"),
                    shown("preview_tilt_offset", "Tilt angle", Shown::Both),
                ],
            },
        ],
    },
    Page {
        name: "About this brush",
        icon: &icons::STUDIO_ABOUT,
        icon_size: 45.0,
        sections: &[Section { title: None, controls: &[Control::About] }],
    },
];

/// Every key the controls read.
fn studio_keys() -> Vec<String> {
    fn add(c: &Control, keys: &mut Vec<String>) {
        match c {
            Control::Slider(key, ..) => {
                keys.push((*key).to_owned());
                let e = slider_extras((*key).to_owned());
                keys.extend([e.tilt_switch, e.tilt_angle].into_iter().filter(|k| !k.is_empty()));
            }
            Control::Toggle(key, ..) | Control::Choice(key, _) | Control::Kinds(key, _) | Control::Picker(key, _) => {
                keys.push((*key).to_owned());
            }
            Control::TaperBar(a, b, c) => keys.extend([a, b, c].map(|k| (*k).to_owned())),
            Control::TiltAngle => keys.push("tilt_angle".into()),
            Control::Roundness => keys.extend(["roundness".to_owned(), "shape_angle".to_owned()]),
            Control::Disclosure(_, inside) => inside.iter().for_each(|c| add(c, keys)),
            _ => {}
        }
    }
    let mut keys = Vec::new();
    for page in PAGES {
        for section in page.sections {
            section.controls.iter().for_each(|c| add(c, &mut keys));
        }
    }
    keys.push("most_size".into());
    keys.dedup();
    keys
}

/// What a space in the settings column can follow or come before, besides a control.
#[derive(Clone, Copy)]
enum Edge<'a> {
    Top,
    Title,
    Control(&'a Control),
}

/// The space between `above` and `below` (a control, or a title when none). Titles are 23 tall,
/// slider rows 55, switch and picker rows 47, list rows 44 with a line between.
fn gap(above: Edge, below: Option<&Control>) -> f32 {
    let line = |c: &Control| matches!(c, Control::Toggle(..) | Control::Picker(..));
    match (above, below) {
        (Edge::Top, _) => 8.0,
        (Edge::Title, Some(b)) => match b {
            b if line(b) => 7.5,
            Control::PressureCurve | Control::About => 29.0,
            Control::TiltAngle => 28.0,
            Control::Disclosure(..) => 41.0,
            _ => 23.0,
        },
        (Edge::Title, None) => 23.0,
        (Edge::Control(a), None) => match a {
            a if line(a) => 50.5,
            Control::Slider(..) => 46.5,
            Control::PreviewCard => 48.0,
            _ => 47.0,
        },
        (Edge::Control(a), Some(b)) => match a {
            a if line(a) => {
                if line(b) {
                    4.0
                } else {
                    19.5
                }
            }
            Control::Slider(..) | Control::Choice(..) => {
                if line(b) {
                    0.0
                } else {
                    15.5
                }
            }
            Control::Kinds(..) => 16.0,
            Control::TaperBar(..) => 23.5,
            Control::PressureCurve => 24.5,
            Control::TiltAngle => 21.5,
            Control::Roundness => 16.5,
            Control::Disclosure(..) => 29.0,
            _ => 15.5,
        },
    }
}

/// A slider's value as it reads.
fn shown_value(v: f32, shown: Shown, most: f32) -> String {
    let pc = (v * 100.0).round() as i32;
    // between ends that read by name, a value never rounds onto an end (0.996 reads 99%)
    let between = format!("{}%", pc.clamp(1, 99));
    let below_top = format!("{}%", pc.min(99));
    let named = |low: &str, high: &str| {
        if v <= 0.0 {
            low.to_owned()
        } else if v >= 1.0 {
            high.to_owned()
        } else {
            between.clone()
        }
    };
    match shown {
        Shown::Percent => named("None", "Max"),
        Shown::Both => {
            if v >= 1.0 {
                "Max".into()
            } else {
                below_top
            }
        }
        Shown::Grade => {
            if v >= 1.0 {
                "Smooth".into()
            } else {
                below_top
            }
        }
        Shown::Double => format!("{}%", (v * 200.0).round() as i32),
        Shown::Twice => {
            if v <= 0.0 {
                "None".into()
            } else if v >= 1.0 {
                "Max".into()
            } else {
                format!("{}%", ((v * 200.0).round() as i32).clamp(1, 199))
            }
        }
        Shown::Follow => {
            if v >= 1.0 {
                "Follow stroke".into()
            } else {
                below_top
            }
        }
        Shown::Count => format!("{}", 1 + (v * 15.0).round() as i32),
        Shown::Tip => named("Sharp", "Blunt"),
        Shown::Movement => named("Stamp", "Rolling"),
        Shown::Zoom => named("Follow size", "Cropped"),
        Shown::Charge => named("Disabled", "Max"),
        Shown::Metal => named("Non-Metallic", "Metallic"),
        Shown::Gloss => named("Glossy", "Matte"),
        Shown::Size => {
            let size = most * v.clamp(0.0, 1.0).powf(2.2);
            // Max only at the control's end: a size of more than a unit reads as its percent
            if v >= 1.0 {
                "Max".into()
            } else if size <= 0.0 {
                "None".into()
            } else {
                format!("{}%", (size * 100.0).round() as i32)
            }
        }
    }
}

/// What the studio asks of the app.
pub enum Asked {
    /// Closed (kept or not): the library shows the brush as it now is.
    Closed,
}

#[derive(Default)]
pub struct Studio {
    pub open: bool,
    tab: usize,
    name: String,
    values: HashMap<String, f32>,
    /// A new brush: cancelled, it goes.
    new_brush: Option<(u32, u32)>,
    /// The drawing pad: its canvas, where it shows (points), its picture, whether its first
    /// frame was drawn, its colour, and a stroke on it.
    pub pad: Option<Arc<CanvasSession>>,
    pub pad_rect: Option<Rect>,
    pub pad_texture: Option<TextureId>,
    pad_ready: bool,
    pad_colour: usize,
    pad_stroke: bool,
    /// The pen painting on the pad, if one is: its pointer and pressure as last given.
    pad_pen: Option<(u64, f32)>,
    pad_menu: bool,
    /// The frame the pad menu opened in (the press that opened it doesn't close it).
    pad_menu_frame: u64,
    /// The brush's sample stroke, and when it is drawn again.
    preview: Option<TextureHandle>,
    preview_due: Option<Instant>,
    /// The source images, by kind; the Disclosure cards open.
    sources: HashMap<String, TextureHandle>,
    open_cards: HashSet<&'static str>,
    /// The Stylus page's pressure curve and its editor.
    curve_points: Vec<[f32; 2]>,
    curve: crate::curve::CurveEditor,
    /// A slider's keypad, and a blend mode's list (its key, where its row is, and whether it just
    /// opened), while open.
    keypad: Option<Keypad>,
    blend_list: Option<(&'static str, f32, bool)>,
    /// A wide window (the pages' card, sources as squares); the brush's About; the pad shown as a
    /// sphere (and since when it was told).
    wide: bool,
    about: Option<AboutShown>,
    sphere: Option<bool>,
    started: Option<Instant>,
    /// A source's editor, while open.
    source_editor: Option<crate::source_editor::SourceEditor>,
}

impl Studio {
    /// Opens the studio on the current tool's brush (`new`: one just made, which Cancel throws
    /// away).
    pub fn begin(&mut self, ctx: &egui::Context, session: &CanvasSession, new: Option<(u32, u32)>) {
        self.name = session.studio_begin();
        self.new_brush = new;
        // white on the dark pad, black on the light
        self.pad_colour = if crate::look::light() { PAD_BLACK } else { PAD_WHITE };
        self.open = true;
        self.pad_menu = false;
        self.source_editor = None;
        self.open_cards.clear();
        self.started = Some(Instant::now());
        self.read(ctx, session);
    }

    fn read(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        let keys = studio_keys();
        let values = session.studio_settings(keys.clone());
        self.values = keys.into_iter().zip(values).collect();
        self.read_sources(ctx, session);
        self.about = Some(AboutShown::read(ctx, session));
        let points = session.studio_curve();
        self.curve_points = points.as_chunks::<2>().0.to_vec();
        if self.curve_points.len() < 2 {
            self.curve_points = crate::curve::straight();
        }
        self.preview_due = Some(Instant::now());
    }

    fn read_sources(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        self.sources.clear();
        for kind in [SourceKind::Shape, SourceKind::Grain, SourceKind::Metallic, SourceKind::Roughness] {
            let pixels = session.studio_source(kind);
            let n = (pixels.len() as f64).sqrt() as usize;
            if n == 0 || n * n != pixels.len() {
                continue;
            }
            let rgba: Vec<u8> = pixels.iter().flat_map(|&v| [v, v, v, 255]).collect();
            let image = egui::ColorImage::from_rgba_unmultiplied([n, n], &rgba);
            let texture = ctx.load_texture(format!("source {kind:?}"), image, egui::TextureOptions::LINEAR);
            self.sources.insert(format!("{kind:?}"), texture);
        }
    }

    fn value(&self, key: &str) -> f32 {
        self.values.get(key).copied().unwrap_or(0.0)
    }

    fn set(&mut self, session: &Arc<CanvasSession>, key: &str, value: f32) {
        self.values.insert(key.to_owned(), value);
        session.set_studio_setting(key.to_owned(), value);
        // the spacing sets how big the brush may be, so where the size controls sit
        if key == "spacing" {
            let keys = ["max_size", "min_size", "most_size"].map(str::to_owned).to_vec();
            for (k, v) in keys.clone().into_iter().zip(session.studio_settings(keys)) {
                self.values.insert(k, v);
            }
        }
        self.preview_due = Some(Instant::now() + PREVIEW_DELAY);
        self.sync_pad(session);
    }

    /// The pad's canvas is made: dark, painting with the brush being edited, keeping its strokes.
    pub fn start_pad(&mut self, pad: Arc<CanvasSession>, session: &Arc<CanvasSession>) {
        let back = if crate::look::light() { PAD_BACK_LIGHT } else { PAD_BACK };
        pad.set_background_color(back, back, back, true);
        pad.keep_strokes();
        self.pad = Some(pad);
        self.pad_ready = false;
        self.sync_pad(session);
    }

    /// The pad paints with the brush as it is now, and what is on it is drawn again that way.
    fn sync_pad(&self, session: &Arc<CanvasSession>) {
        let Some(pad) = &self.pad else { return };
        let c = PAD_COLOURS.get(self.pad_colour).copied().unwrap_or(0xFFFFFF);
        let unit = |shift: u32| ((c >> shift) & 0xFF) as f32 / 255.0;
        // drawn again on the core's own thread: a slider dragged doesn't wait for it
        Arc::clone(pad).sync_pad(Arc::clone(session), unit(16), unit(8), unit(0));
    }

    /// Closes the studio: `keep` keeps the edits, otherwise the brush goes back to how it opened
    /// (a new brush cancelled goes).
    fn close(&mut self, session: &CanvasSession, keep: bool) {
        session.studio_end(keep);
        if !keep && let Some((set, index)) = self.new_brush {
            session.delete_brush(set, index);
        }
        self.new_brush = None;
        self.open = false;
        self.pad = None;
        self.pad_texture = None;
        self.pad_menu = false;
        self.source_editor = None;
        self.preview = None;
    }

    /// Shows the studio over everything while open.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &Arc<CanvasSession>,
        prefs: &mut crate::prefs::Prefs,
    ) -> Option<Asked> {
        if !self.open {
            return None;
        }
        if self.preview_due.is_some_and(|due| Instant::now() >= due) {
            self.preview_due = None;
            self.preview = brush_preview(ctx, session);
        } else if let Some(due) = self.preview_due {
            ctx.request_repaint_after(due.saturating_duration_since(Instant::now()));
        }
        let screen = ctx.content_rect();
        let wide = screen.width() > screen.height();
        self.wide = wide;
        // a sphere showing the brush's materials on their page, or on every page with the 3D
        // preview on
        let sphere = PAGES.get(self.tab).is_some_and(|p| p.name == "Materials") || prefs.pad_3d;
        if self.pad_ready
            && self.sphere != Some(sphere)
            && let Some(pad) = &self.pad
        {
            pad.show_sphere(sphere);
            self.sphere = Some(sphere);
        }
        let mut asked = None;
        egui::Area::new(Id::unique("brush studio")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
            ui.allocate_rect(screen, Sense::click_and_drag());
            ui.painter().rect_filled(screen, 0, argb(BACK));
            let (pages, settings, pad) = if wide {
                let card = Rect::from_min_max(
                    screen.min + vec2(10.0, 10.0),
                    pos2(screen.left() + 330.0, screen.bottom() - 10.0),
                );
                let settings = Rect::from_min_max(
                    pos2(card.right(), screen.top()),
                    pos2(card.right() + SETTINGS_WIDTH, screen.bottom()),
                );
                let line = Rect::from_min_max(
                    pos2(settings.right(), screen.top()),
                    pos2(settings.right() + 1.0, screen.bottom()),
                );
                ui.painter().rect_filled(line, 0, argb(LINE));
                (card, settings, Rect::from_min_max(pos2(line.right(), screen.top()), screen.max))
            } else {
                let rail = Rect::from_min_max(screen.min, pos2(screen.left() + TABS_WIDTH, screen.bottom()));
                let pad = Rect::from_min_max(
                    pos2(rail.right(), screen.top()),
                    pos2(screen.right(), screen.top() + PAD_HEIGHT),
                );
                let line =
                    Rect::from_min_max(pos2(rail.right(), pad.bottom()), pos2(screen.right(), pad.bottom() + 1.0));
                ui.painter().rect_filled(line, 0, argb(LINE));
                (rail, Rect::from_min_max(pos2(rail.right(), line.bottom()), screen.max), pad)
            };
            if wide {
                self.pages_card(ui, look, pages);
            } else {
                self.rail(ui, look, pages);
            }
            self.settings(ui, look, session, settings);
            if let Some(done) = self.pad_view(ui, look, pad) {
                self.close(session, done);
                asked = Some(Asked::Closed);
            }
        });
        if self.pad_menu {
            self.pad_menu(ctx, session, prefs);
        }
        if let Some(mut keypad) = self.keypad.take()
            && keypad.show(ctx, look, session, self)
        {
            self.keypad = Some(keypad);
        }
        self.blend_list_view(ctx, look, session);
        let mut curve = std::mem::take(&mut self.curve_points);
        if self.curve.menu(ctx, &mut curve) {
            session.set_studio_curve(curve.iter().flatten().copied().collect());
            self.preview_due = Some(Instant::now() + PREVIEW_DELAY);
            self.sync_pad(session);
        }
        self.curve_points = curve;
        if let Some(editor) = self.source_editor.as_mut()
            && let Some(changed) = editor.show(ctx, look, session, wide)
        {
            self.source_editor = None;
            if changed {
                self.read_sources(ctx, session);
                self.changed(session);
            }
        }
        asked
    }

    /// Wide: the pages by icon and name on a card headed "Brush Studio".
    fn pages_card(&mut self, ui: &mut Ui, look: &mut Look, card: Rect) {
        ui.painter().rect_filled(card, 20, argb(TABS_FILL));
        ui.painter().rect_stroke(card, 20, Stroke::new(1.0, argb(CARD_EDGE)), egui::StrokeKind::Inside);
        let at = pos2(card.left() + 24.0, card.top() + 19.0);
        text(ui.painter(), at, Align2::LEFT_TOP, "Brush Studio", 23.0, Weight::Bold, argb(CARD_TITLE));
        let mut y = card.top() + 19.0 + 29.0 + 17.0;
        for (i, page) in PAGES.iter().enumerate() {
            let row = Rect::from_min_size(pos2(card.left() + 8.0, y), vec2(card.width() - 16.0, 44.0));
            let on = self.tab == i;
            if on {
                ui.painter().rect_filled(row, 22, argb(pc::ACCENT));
            }
            let ink = if on { Color32::WHITE } else { argb(TAB_ICON) };
            look.icon(ui, page.icon, pos2(row.left() + 26.0, row.center().y), page.icon_size, ink);
            let ink = if on { Color32::WHITE } else { argb(PAGE_INK) };
            let at = pos2(row.left() + 52.0, row.center().y);
            text(ui.painter(), at, Align2::LEFT_CENTER, page.name, 15.5, Weight::Regular, ink);
            if ui.interact(row, ui.scope_id().with(("page", i)), Sense::click()).clicked() {
                self.tab = i;
            }
            y += 44.0 + 4.0;
        }
    }

    /// Upright: the pages as icons down the left.
    fn rail(&mut self, ui: &mut Ui, look: &mut Look, rail: Rect) {
        ui.painter().rect_filled(rail, 0, argb(TABS_FILL));
        let mut y = rail.top() + 46.0;
        for (i, page) in PAGES.iter().enumerate() {
            let cell = Rect::from_min_size(pos2(rail.left() + 8.0, y), vec2(65.0, 45.0));
            let on = self.tab == i;
            if on {
                ui.painter().rect_filled(cell, 22.5, argb(pc::ACCENT));
            }
            let ink = if on { Color32::WHITE } else { argb(TAB_ICON) };
            look.icon(ui, page.icon, cell.center(), page.icon_size, ink);
            if ui.interact(cell, ui.scope_id().with(("page", i)), Sense::click()).clicked() {
                self.tab = i;
            }
            y += 45.0 + 3.0;
        }
    }

    /// The open page's settings, in sections, spaced as the Android app spaces them.
    fn settings(&mut self, ui: &mut Ui, look: &mut Look, session: &Arc<CanvasSession>, area: Rect) {
        let Some(page) = PAGES.get(self.tab) else { return };
        let column = Rect::from_min_max(pos2(area.left() + 20.0, area.top()), pos2(area.right() - 20.0, area.bottom()));
        ui.scope_builder(UiBuilder::new().max_rect(column), |ui| {
            egui::ScrollArea::vertical()
                .id_salt(("studio page", self.tab))
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    let width = ui.available_width();
                    let mut above = Edge::Top;
                    for (k, section) in page.sections.iter().enumerate() {
                        // the About page is headed by the brush's name
                        let title = section.title.map(str::to_owned).or_else(|| {
                            matches!(section.controls.first(), Some(Control::About)).then(|| self.name.clone())
                        });
                        if let Some(title) = title {
                            ui.add_space(if k == 0 { 25.0 } else { gap(above, None) });
                            let (row, _) = ui.allocate_exact_size(vec2(width, 23.0), Sense::hover());
                            let at = pos2(row.left() + 8.0, row.center().y);
                            text(ui.painter(), at, Align2::LEFT_CENTER, &title, 21.0, Weight::Bold, argb(HEADER));
                            above = Edge::Title;
                        }
                        for control in section.controls {
                            ui.add_space(gap(above, Some(control)));
                            self.control(ui, look, session, control, width);
                            above = Edge::Control(control);
                        }
                    }
                    ui.add_space(77.0);
                });
        });
    }

    /// Whether `enabled_by` lets a control be used: the control it names is on that choice.
    fn enabled(&self, by: Option<(&str, i32)>) -> bool {
        by.is_none_or(|(key, choice)| self.value(key).round() as i32 == choice)
    }

    fn control(&mut self, ui: &mut Ui, look: &mut Look, session: &Arc<CanvasSession>, c: &'static Control, width: f32) {
        match c {
            Control::Slider(key, label, how, by) => {
                let enabled = self.enabled(*by);
                let most = if *how == Shown::Size { self.value("most_size") } else { 16.0 };
                let (row, _) = ui.allocate_exact_size(vec2(width, 55.0), Sense::hover());
                let typing = self.keypad.as_ref().is_some_and(|k| k.key == *key);
                let hit = studio_slider(ui, row, label, self.value(key), *how, most, enabled, key, typing);
                if let Some(v) = hit.set {
                    self.set(session, key, v);
                }
                if let Some(pill) = hit.pill {
                    self.keypad = Some(Keypad::open(ui.ctx(), session, key, label, *how, most, pill));
                }
            }
            Control::Toggle(key, label, by) => {
                let enabled = self.enabled(*by);
                let (row, _) = ui.allocate_exact_size(vec2(width, 47.0), Sense::hover());
                let ink = if enabled { argb(DIM) } else { argb(OFF) };
                text(
                    ui.painter(),
                    pos2(row.left() + 8.0, row.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    17.0,
                    Weight::Regular,
                    ink,
                );
                let on = self.value(key) >= 0.5;
                let at = pos2(row.right() - 6.0 - 63.0, row.center().y - 14.0);
                if enabled {
                    crate::transform::switch(ui, at, on, key);
                    let hit = Rect::from_min_size(at, vec2(63.0, 28.0));
                    if ui.interact(hit, ui.scope_id().with(("toggle", key)), Sense::click()).clicked() {
                        self.set(session, key, if on { 0.0 } else { 1.0 });
                    }
                } else {
                    let track = Rect::from_min_size(at, vec2(63.0, 28.0));
                    ui.painter().rect_filled(track, 14, argb(SWITCH_FIXED));
                    let knob = Rect::from_min_size(at + vec2(2.0 + if on { 22.0 } else { 0.0 }, 2.0), vec2(37.0, 24.0));
                    ui.painter().rect_filled(knob, 12, argb(SWITCH_FIXED_KNOB));
                }
            }
            Control::Choice(key, options) => {
                let chosen = self.value(key).round() as i32;
                let height = 44.0 * options.len() as f32 + options.len().saturating_sub(1) as f32;
                let (group, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
                ui.painter().rect_filled(group, 22, argb(LIST));
                let mut y = group.top();
                for (i, label) in options.iter().enumerate() {
                    if i > 0 {
                        ui.painter().rect_filled(
                            Rect::from_min_size(pos2(group.left(), y), vec2(width, 1.0)),
                            0,
                            argb(LIST_LINE),
                        );
                        y += 1.0;
                    }
                    let row = Rect::from_min_size(pos2(group.left(), y), vec2(width, 44.0));
                    let on = i as i32 == chosen;
                    if on {
                        let corner = |at: bool| if at { 22 } else { 0 };
                        let corners = egui::CornerRadius {
                            nw: corner(i == 0),
                            ne: corner(i == 0),
                            sw: corner(i + 1 == options.len()),
                            se: corner(i + 1 == options.len()),
                        };
                        ui.painter().rect_filled(row, corners, argb(pc::ACCENT));
                        look.icon(
                            ui,
                            &icons::MENU_TICK,
                            pos2(row.right() - 14.0 - 17.0, row.center().y),
                            34.0,
                            Color32::WHITE,
                        );
                    }
                    let ink = if on { Color32::WHITE } else { argb(DIM) };
                    text(
                        ui.painter(),
                        pos2(row.left() + 16.0, row.center().y),
                        Align2::LEFT_CENTER,
                        label,
                        17.0,
                        Weight::Regular,
                        ink,
                    );
                    if ui.interact(row, ui.scope_id().with(("choice", key, i)), Sense::click()).clicked() {
                        self.set(session, key, i as f32);
                    }
                    y += 44.0;
                }
            }
            Control::Kinds(key, options) => {
                let chosen = self.value(key).round() as i32;
                let (band, _) = ui.allocate_exact_size(vec2(width, 34.0), Sense::hover());
                let track = band.shrink2(vec2(8.0, 0.0));
                ui.painter().rect_filled(track, 17, argb(SEGMENTS));
                let n = options.len().max(1) as f32;
                let item = (track.width() - 4.0 - 11.0 * (n - 1.0)) / n;
                for (i, (icon, label)) in options.iter().enumerate() {
                    let cell =
                        Rect::from_min_size(track.min + vec2(2.0 + (item + 11.0) * i as f32, 2.0), vec2(item, 30.0));
                    let on = i as i32 == chosen;
                    if on {
                        ui.painter().rect_filled(cell, 15, argb(pc::ACCENT));
                    }
                    let ink = if on { Color32::WHITE } else { argb(DIM) };
                    let galley = ui.painter().layout_no_wrap((*label).to_owned(), Weight::Regular.font(13.0), ink);
                    let left = cell.center().x - (30.0 + 2.0 + galley.size().x) / 2.0;
                    look.icon(ui, icon, pos2(left + 15.0, cell.center().y), 30.0, ink);
                    ui.painter().galley(pos2(left + 32.0, cell.center().y - galley.size().y / 2.0), galley, ink);
                    if ui.interact(cell, ui.scope_id().with(("kind", key, i)), Sense::click()).clicked() {
                        self.set(session, key, i as f32);
                    }
                }
            }
            Control::Source(label, kind) => {
                let image = self.sources.get(&format!("{kind:?}")).map(|t| t.id());
                let material = matches!(kind, SourceKind::Metallic | SourceKind::Roughness);
                if self.wide {
                    // a title with an Edit button over a square of its image as wide as the rows
                    let (head, _) = ui.allocate_exact_size(vec2(width, 8.5 + 45.0), Sense::hover());
                    let at = pos2(head.left() + 8.0, head.top() + 8.5 + 9.5);
                    text(ui.painter(), at, Align2::LEFT_TOP, label, 21.0, Weight::Bold, argb(HEADER));
                    let edit = Rect::from_min_size(pos2(head.right() + 4.0 - 54.0, head.top() + 8.5), vec2(54.0, 45.0));
                    let mut open =
                        ui.interact(edit, ui.scope_id().with(("edit source", label)), Sense::click()).clicked();
                    ui.painter().rect_filled(edit, 22.5, argb(BUTTON));
                    text(
                        ui.painter(),
                        edit.center(),
                        Align2::CENTER_CENTER,
                        "Edit",
                        17.0,
                        Weight::Regular,
                        argb(BUTTON_INK),
                    );
                    ui.add_space(12.5);
                    let (square, response) = ui.allocate_exact_size(vec2(width, width), Sense::click());
                    open |= response.clicked();
                    let white = image.is_none() && material;
                    ui.painter().rect_filled(square, 22, if white { Color32::WHITE } else { argb(BUTTON) });
                    match image {
                        Some(id) => {
                            let shape = egui::epaint::RectShape::filled(square, 22, Color32::WHITE);
                            ui.painter().add(shape.with_texture(id, unit_uv()));
                        }
                        // the round tip: a white disc
                        None if !white => _ = ui.painter().circle_filled(square.center(), width * 0.35, Color32::WHITE),
                        None => {}
                    }
                    if open {
                        self.source_editor = Some(crate::source_editor::SourceEditor::new(*kind, session));
                    }
                } else {
                    let (row, response) = ui.allocate_exact_size(vec2(width, 60.0), Sense::click());
                    if response.clicked() {
                        self.source_editor = Some(crate::source_editor::SourceEditor::new(*kind, session));
                    }
                    ui.painter().rect_filled(row, 12, argb(CARD));
                    let at = pos2(row.left() + 16.0, row.center().y);
                    text(ui.painter(), at, Align2::LEFT_CENTER, label, 21.0, Weight::Bold, argb(HEADER));
                    let picture =
                        Rect::from_min_size(pos2(row.right() - 8.0 - 44.0, row.center().y - 22.0), vec2(44.0, 44.0));
                    match image {
                        Some(id) => _ = ui.painter().image(id, picture, unit_uv(), Color32::WHITE),
                        // the round tip a white disc; a material's amount all over white
                        None if material => _ = ui.painter().rect_filled(picture, 10, Color32::WHITE),
                        None => _ = ui.painter().circle_filled(picture.center(), 22.0, Color32::WHITE),
                    }
                }
            }
            Control::Picker(key, label) => {
                let (row, _) = ui.allocate_exact_size(vec2(width, 47.0), Sense::hover());
                text(
                    ui.painter(),
                    pos2(row.left() + 8.0, row.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    17.0,
                    Weight::Regular,
                    argb(DIM),
                );
                let mode = blend_name(key, self.value(key));
                let at = pos2(row.right() - 8.0, row.center().y);
                let shown =
                    text(ui.painter(), at, Align2::RIGHT_CENTER, &mode, 17.0, Weight::Regular, argb(pc::ACCENT));
                if ui.interact(shown.expand(6.0), ui.scope_id().with(("blend", key)), Sense::click()).clicked() {
                    self.blend_list = Some((key, row.center().y, true));
                }
            }
            Control::Disclosure(label, inside) => {
                let open = self.open_cards.contains(label);
                let inner_height = if open {
                    let mut h = 12.0 + 8.0;
                    let mut above: Option<&Control> = None;
                    for c in inside.iter() {
                        if let Some(a) = above {
                            h += gap(Edge::Control(a), Some(c));
                        }
                        h += self.control_height(c, width - 16.0);
                        above = Some(c);
                    }
                    h
                } else {
                    0.0
                };
                let (card, _) = ui.allocate_exact_size(vec2(width, 60.0 + inner_height), Sense::hover());
                ui.painter().rect_filled(card, 20, argb(MATERIAL));
                let head = Rect::from_min_size(card.min, vec2(width, 60.0));
                let at = pos2(head.left() + 16.0, head.center().y);
                text(ui.painter(), at, Align2::LEFT_CENTER, label, 17.0, Weight::SemiBold, argb(MATERIAL_INK));
                let disc = pos2(head.right() - 16.0 - 15.0, head.center().y);
                ui.painter().circle_filled(disc, 15.0, argb(BACK));
                let chevron = if open { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT };
                look.icon(ui, chevron, disc, 16.0, Color32::WHITE);
                if ui.interact(head, ui.scope_id().with(("card", label)), Sense::click()).clicked() {
                    if open {
                        self.open_cards.remove(label);
                    } else {
                        self.open_cards.insert(label);
                    }
                }
                if open {
                    let inner = Rect::from_min_max(
                        pos2(card.left() + 8.0, head.bottom() + 12.0),
                        pos2(card.right() - 8.0, card.bottom() - 8.0),
                    );
                    ui.scope_builder(UiBuilder::new().max_rect(inner), |ui| {
                        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                        let mut above: Option<&Control> = None;
                        for c in inside.iter() {
                            if let Some(a) = above {
                                ui.add_space(gap(Edge::Control(a), Some(c)));
                            }
                            self.control(ui, look, session, c, inner.width());
                            above = Some(c);
                        }
                    });
                }
            }
            Control::PreviewCard => {
                let (card, _) = ui.allocate_exact_size(vec2(width, width * 91.5 / 278.5), Sense::hover());
                ui.painter().rect_filled(card, 12, argb(PREVIEW_FILL));
                if let Some(t) = &self.preview {
                    // the stroke filling the card's height, cropped at its sides
                    let [w, h] = t.size().map(|v| v.max(1) as f32);
                    let k = card.height() / h;
                    let shown_w = (card.width() / (w * k)).min(1.0);
                    let uv = Rect::from_min_max(pos2(0.5 - shown_w / 2.0, 0.0), pos2(0.5 + shown_w / 2.0, 1.0));
                    ui.painter().image(t.id(), card, uv, Color32::WHITE);
                }
                ui.painter().rect_stroke(card, 12, Stroke::new(1.0, argb(PREVIEW_EDGE)), egui::StrokeKind::Inside);
                let galley =
                    ui.painter().layout_no_wrap(self.name.clone(), Weight::Bold.font(20.0), argb(PREVIEW_NAME));
                let patch = Rect::from_min_size(card.min + vec2(15.5, 14.5), vec2(galley.size().x + 34.0, 37.0));
                ui.painter().rect_filled(patch, 10, argb(PREVIEW_FILL));
                ui.painter().galley(
                    pos2(patch.left() + 17.0, patch.center().y - galley.size().y / 2.0),
                    galley,
                    argb(PREVIEW_NAME),
                );
            }
            Control::TaperBar(start, end, link) => {
                let (bar, _) = ui.allocate_exact_size(vec2(width, 72.0), Sense::hover());
                // linked, the two ends move together
                let linked = self.value(link) >= 0.5;
                let (s0, e0) = (self.value(start), self.value(end));
                if let Some((at_start, v)) = taper_bar(ui, bar, s0, e0, start) {
                    let (key, other) = if at_start { (start, end) } else { (end, start) };
                    self.set(session, key, v);
                    if linked {
                        self.set(session, other, v);
                    }
                }
                ui.add_space(4.5);
                if let Some(link) = LINK_TIPS.get(usize::from(*start == "touch_taper_start")) {
                    self.control(ui, look, session, link, width);
                }
            }
            Control::PressureCurve => {
                let (band, _) = ui.allocate_exact_size(vec2(width, width - 16.0), Sense::hover());
                let square = band.shrink2(vec2(8.0, 0.0));
                let mut curve = std::mem::take(&mut self.curve_points);
                if self.curve.show(ui, square, &mut curve, &crate::curve::STUDIO, "studio") {
                    session.set_studio_curve(curve.iter().flatten().copied().collect());
                    self.preview_due = Some(Instant::now() + PREVIEW_DELAY);
                    self.sync_pad(session);
                }
                self.curve_points = curve;
            }
            Control::TiltAngle => {
                let (band, _) = ui.allocate_exact_size(vec2(width, (width - 10.0) * 221.0 / 271.5), Sense::hover());
                if let Some(v) =
                    tilt_angle(ui, band.shrink2(vec2(5.0, 0.0)), self.value("tilt_angle"), &DARK_TILT, true)
                {
                    self.set(session, "tilt_angle", v);
                }
            }
            Control::Roundness => {
                let (graph, _) = ui.allocate_exact_size(vec2(width, width * 231.5 / 283.0), Sense::hover());
                let (round, angle) = (self.value("roundness"), self.value("shape_angle"));
                match roundness_graph(ui, graph, round, angle) {
                    Some(Turned::Round(v)) => self.set(session, "roundness", v),
                    Some(Turned::Angle(v)) => self.set(session, "shape_angle", v),
                    None => {}
                }
            }
            Control::About => self.about_view(ui, look, session, width),
        }
    }

    /// The drawing pad with its buttons: Drawing Pad (its menu), Cancel and Done. Returns whether
    /// Done (true) or Cancel (false) was tapped.
    fn pad_view(&mut self, ui: &mut Ui, look: &mut Look, area: Rect) -> Option<bool> {
        self.pad_rect = Some(area);
        ui.painter().rect_filled(area, 0, argb(BACK));
        if let Some(id) = self.pad_texture {
            ui.painter().image(id, area, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        // the pad's first frame drawn: its sample scribble
        if !self.pad_ready
            && let Some(pad) = &self.pad
            && self.pad_texture.is_some()
            && pad.info().frames > 0
        {
            let sphere = PAGES.get(self.tab).is_some_and(|p| p.name == "Materials");
            pad.show_sphere(sphere || self.sphere == Some(true));
            pad.draw_sample_scribble();
            self.pad_ready = true;
            self.sphere = None;
        }
        let response = ui.interact(area, ui.scope_id().with("pad"), Sense::click_and_drag());
        self.pad_input(ui, &response, area);
        // Drawing Pad
        let galley =
            ui.painter().layout_no_wrap("Drawing Pad".to_owned(), Weight::SemiBold.font(16.0), argb(BUTTON_INK));
        let button =
            Rect::from_min_size(area.min + vec2(21.0, 21.0), vec2(9.0 + 26.0 + 6.0 + galley.size().x + 13.0, 43.0));
        ui.painter().rect_filled(button, 21.5, argb(BUTTON));
        ui.painter().rect_stroke(button, 21.5, Stroke::new(1.0, argb(BUTTON_EDGE)), egui::StrokeKind::Inside);
        look.icon(ui, &icons::STUDIO_PAD, pos2(button.left() + 9.0 + 13.0, button.center().y), 26.0, argb(BUTTON_INK));
        ui.painter().galley(
            pos2(button.left() + 41.0, button.center().y - galley.size().y / 2.0),
            galley,
            argb(BUTTON_INK),
        );
        if ui.interact(button, ui.scope_id().with("pad menu"), Sense::click()).clicked() {
            self.pad_menu = !self.pad_menu;
            self.pad_menu_frame = ui.ctx().cumulative_frame_nr();
        }
        // Cancel and Done at the top right
        let done = Rect::from_min_size(pos2(area.right() - 20.0 - 44.0, area.top() + 20.0), vec2(44.0, 44.0));
        let cancel = Rect::from_min_size(pos2(done.left() - 13.0 - 73.0, done.top()), vec2(73.0, 43.0));
        ui.painter().rect_filled(cancel, 21.5, argb(BUTTON));
        ui.painter().rect_stroke(cancel, 21.5, Stroke::new(1.0, argb(BUTTON_EDGE)), egui::StrokeKind::Inside);
        text(ui.painter(), cancel.center(), Align2::CENTER_CENTER, "Cancel", 17.0, Weight::Regular, argb(BUTTON_INK));
        ui.painter().circle_filled(done.center(), 22.0, argb(DONE));
        look.icon(ui, &icons::DONE_TICK, done.center(), 40.0, Color32::WHITE);
        if ui.interact(cancel, ui.scope_id().with("cancel"), Sense::click()).clicked() {
            return Some(false);
        }
        if ui.interact(done, ui.scope_id().with("done"), Sense::click()).clicked() {
            return Some(true);
        }
        None
    }

    /// A drag on the pad paints on it (the first stroke of one's own taking the sample
    /// scribble's place).
    fn pad_input(&mut self, ui: &Ui, response: &egui::Response, area: Rect) {
        let Some(pad) = &self.pad else { return };
        let ppp = ui.ctx().pixels_per_point();
        let ms = self.started.map_or(0.0, |s| s.elapsed().as_secs_f64() * 1000.0);
        // a pen paints on the pad as on the canvas: its pressure, lean, roll and eraser end (a
        // pen's touches carry a force; under X11 a tablet's pen moves the mouse pointer), and every
        // one of its moves, not only where it is once a frame (a brush that turns with the pen
        // would turn in steps)
        let mut moves = Vec::new();
        ui.input(|i| {
            for event in &i.events {
                // (a pen landing may come without a force: its own pressure is taken)
                if let egui::Event::Touch { id, force, phase, pos, .. } = event
                    && matches!(phase, egui::TouchPhase::Start | egui::TouchPhase::Move)
                    && crate::pen::is_pen(id.0) != Some(false)
                    && let Some(force) = force.or_else(|| crate::pen::pressure(id.0))
                {
                    self.pad_pen = Some((id.0, force));
                    moves.push((*pos, force));
                }
            }
        });
        let pen = self.pad_pen.or_else(|| crate::pen::mouse_pen().map(|m| (MOUSE_POINTER, m.pressure)));
        let sample = |p: Pos2, pressure: Option<f32>| {
            let local = (p - area.min) * ppp;
            let (pressure, (tilt, orientation), roll) = pen.map_or((1.0, (0.0, 0.0), 0.0), |(id, given)| {
                let lean = crate::pen::lean(id).unwrap_or((0.0, 0.0));
                (pressure.unwrap_or(given), lean, crate::pen::roll(id).unwrap_or(0.0))
            });
            PenSample { x: local.x, y: local.y, pressure, tilt, orientation, time_ms: ms, roll }
        };
        let kind = match pen {
            Some((MOUSE_POINTER, _)) if crate::pen::mouse_pen().is_some_and(|m| m.eraser) => PointerKind::StylusEraser,
            Some((id, _)) if id != MOUSE_POINTER && crate::pen::eraser_end(id) => PointerKind::StylusEraser,
            Some(_) => PointerKind::Stylus,
            None => PointerKind::Mouse,
        };
        let Some(p) = response.interact_pointer_pos() else { return };
        if response.drag_started() {
            self.pad_menu = false;
            pad.begin_own_stroke();
            pad.stroke_begin(kind, vec![sample(p, None)]);
            self.pad_stroke = true;
        } else if self.pad_stroke && response.dragged() {
            let samples: Vec<PenSample> = if moves.is_empty() {
                vec![sample(p, None)]
            } else {
                moves.iter().map(|&(at, force)| sample(at, Some(force))).collect()
            };
            pad.stroke_add(samples);
        }
        if self.pad_stroke && response.drag_stopped() {
            pad.stroke_end(vec![sample(p, None)]);
            self.pad_stroke = false;
        }
        if !self.pad_stroke {
            self.pad_pen = None;
        }
        if self.pad_stroke {
            ui.ctx().request_repaint();
        }
    }

    /// The pad's menu: the pad's colour, clearing it, and resetting the brush.
    fn pad_menu(&mut self, ctx: &egui::Context, session: &Arc<CanvasSession>, prefs: &mut crate::prefs::Prefs) {
        let screen = ctx.content_rect();
        // under the Drawing Pad button, centred on it but kept 14 in from the screen's sides
        let centre = self.pad_rect.map_or(screen.left(), |r| r.left() + 21.0 + 70.0);
        let left = (centre - 160.0).clamp(screen.left() + 14.0, (screen.right() - 334.0).max(screen.left() + 14.0));
        let height = 17.0 + 27.0 + 18.0 + 20.0 + 28.0 + 18.0 + 37.0 + 18.0 + 91.0 + 10.0 + 83.0 + 10.0;
        let menu = Rect::from_min_size(pos2(left, screen.top() + 79.0), vec2(320.0, height));
        let ink = argb(0xFF393939);
        egui::Area::new(Id::unique("pad menu")).fixed_pos(menu.min).order(Order::Tooltip).show(ctx, |ui| {
            ui.allocate_rect(menu, Sense::click());
            ui.painter().rect_filled(menu, 26, argb(0xFFC1C1C1));
            let (left, right) = (menu.left() + 10.0, menu.right() - 10.0);
            let mut y = menu.top() + 17.0;
            text(ui.painter(), pos2(left + 12.0, y), Align2::LEFT_TOP, "Settings", 22.0, Weight::Bold, ink);
            y += 27.0 + 18.0;
            // the preview's size: a thin slider, jumping to the press
            let size = self.value("preview_size");
            text(
                ui.painter(),
                pos2(left + 10.0, y + 10.0),
                Align2::LEFT_CENTER,
                "Preview size",
                16.0,
                Weight::Regular,
                ink,
            );
            let shown = format!("{}%", (size * 100.0).round() as i32);
            text(ui.painter(), pos2(right - 10.0, y + 10.0), Align2::RIGHT_CENTER, &shown, 16.0, Weight::Regular, ink);
            y += 20.0;
            let bar = Rect::from_min_max(pos2(left + 10.0, y), pos2(right - 10.0, y + 28.0));
            let (cy, x) = (bar.center().y, bar.left() + bar.width() * size.clamp(0.0, 1.0));
            ui.painter()
                .line_segment([pos2(bar.left(), cy), pos2(bar.right(), cy)], Stroke::new(3.0, argb(0xFFA9A9A9)));
            ui.painter().line_segment([pos2(bar.left(), cy), pos2(x, cy)], Stroke::new(3.0, argb(pc::ACCENT)));
            let kx = x.clamp(bar.left() + 14.0, bar.right() - 14.0);
            let knob = Rect::from_center_size(pos2(kx, cy), vec2(28.0, 17.0));
            ui.painter().rect_filled(knob.translate(vec2(0.0, 1.0)), 8.5, Color32::from_black_alpha(0x33));
            ui.painter().rect_filled(knob, 8.5, Color32::WHITE);
            let response = ui.interact(bar, ui.scope_id().with("preview size"), Sense::click_and_drag());
            if (response.is_pointer_button_down_on() || response.clicked())
                && let Some(p) = response.interact_pointer_pos()
            {
                self.set(session, "preview_size", ((p.x - bar.left()) / bar.width()).clamp(0.0, 1.0));
            }
            y = bar.bottom() + 18.0;
            // the pad's colours
            let step = (right - left - 37.0) / (PAD_COLOURS.len() - 1) as f32;
            for (i, c) in PAD_COLOURS.iter().enumerate() {
                let centre = pos2(left + 18.5 + step * i as f32, y + 18.5);
                if self.pad_colour == i {
                    ui.painter().circle_filled(centre, 18.5, argb(0xFFEBEBEB));
                }
                ui.painter().circle_filled(centre, 9.5, argb(0xFF000000 | c));
                let hit = Rect::from_center_size(centre, vec2(37.0, 37.0));
                if ui.interact(hit, ui.scope_id().with(("pad colour", i)), Sense::click()).clicked() {
                    self.pad_colour = i;
                    self.sync_pad(session);
                }
            }
            y += 37.0 + 18.0;
            // Stylus, and the 3D preview
            let group = Rect::from_min_max(pos2(left, y), pos2(right, y + 91.0));
            ui.painter().rect_filled(group, 16, argb(0xFFD3D3D3));
            let line = Rect::from_min_size(pos2(group.left(), group.top() + 45.0), vec2(group.width(), 1.0));
            ui.painter().rect_filled(line, 0, argb(0xFFBDBDBD));
            let at = pos2(group.left() + 16.0, group.top() + 22.5);
            text(ui.painter(), at, Align2::LEFT_CENTER, "Stylus", 17.0, Weight::Regular, ink);
            let row = Rect::from_min_max(pos2(group.left(), line.bottom()), group.max);
            text(
                ui.painter(),
                pos2(row.left() + 16.0, row.center().y),
                Align2::LEFT_CENTER,
                "3D preview",
                17.0,
                Weight::Regular,
                ink,
            );
            crate::transform::switch(
                ui,
                pos2(row.right() - 8.0 - 63.0, row.center().y - 14.0),
                prefs.pad_3d,
                "3d preview",
            );
            if ui.interact(row, ui.scope_id().with("3d preview"), Sense::click()).clicked() {
                prefs.pad_3d = !prefs.pad_3d;
                prefs.keep();
            }
            y = group.bottom() + 10.0;
            // Clear, and Reset all
            let group = Rect::from_min_max(pos2(left, y), pos2(right, y + 83.0));
            ui.painter().rect_filled(group, 16, argb(0xFFD3D3D3));
            let line = Rect::from_min_size(pos2(group.left(), group.top() + 41.0), vec2(group.width(), 1.0));
            ui.painter().rect_filled(line, 0, argb(0xFFBDBDBD));
            for (k, label) in ["Clear drawing pad", "Reset all brush settings"].into_iter().enumerate() {
                let row =
                    Rect::from_min_size(pos2(group.left(), group.top() + 42.0 * k as f32), vec2(group.width(), 41.0));
                text(
                    ui.painter(),
                    pos2(row.left() + 16.0, row.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    17.0,
                    Weight::Regular,
                    ink,
                );
                if ui.interact(row, ui.scope_id().with(("pad row", k)), Sense::click()).clicked() {
                    if k == 0 {
                        if let Some(pad) = &self.pad {
                            pad.forget_strokes();
                            pad.clear_layer();
                        }
                    } else {
                        session.studio_reset();
                        self.read(ctx, session);
                        self.sync_pad(session);
                    }
                    self.pad_menu = false;
                }
            }
        });
        let pressed_elsewhere =
            ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !menu.contains(p)));
        if pressed_elsewhere && ctx.cumulative_frame_nr() != self.pad_menu_frame {
            self.pad_menu = false;
        }
    }

    /// The brush's About page: a card with its maker's picture, name and signature, then making a
    /// new reset point and resetting to it, with when it was made.
    fn about_view(&mut self, ui: &mut Ui, look: &mut Look, session: &Arc<CanvasSession>, width: f32) {
        let Some(about) = &self.about else { return };
        let (card, _) = ui.allocate_exact_size(vec2(width, 295.0), Sense::hover());
        ui.painter().rect_filled(card, 20, argb(ABOUT_CARD));
        let face = pos2(card.center().x, card.top() + 30.0 + 48.0);
        ui.painter().circle_filled(face, 48.0, argb(ABOUT_FACE));
        match &about.picture {
            Some(t) => circle_image(ui, t.id(), face, 48.0),
            None => look.icon(ui, &icons::BRUSH, face, 56.0, argb(DIM)),
        }
        let name = format!("Made by {}", if about.author.trim().is_empty() { "ProCraft" } else { about.author.trim() });
        let at = pos2(card.center().x, face.y + 48.0 + 21.0 + 12.0);
        text(ui.painter(), at, Align2::CENTER_CENTER, &name, 20.0, Weight::Regular, argb(ABOUT_NAME));
        // the signature in grey, whatever its own colour
        if let Some(t) = &about.signature {
            let room = Rect::from_center_size(pos2(card.center().x, at.y + 12.0 + 25.0 + 32.0), vec2(300.0, 64.0));
            let [w, h] = t.size().map(|v| v.max(1) as f32);
            let k = (room.width() / w).min(room.height() / h);
            let shown = Rect::from_center_size(room.center(), vec2(w * k, h * k));
            ui.painter().image(t.id(), shown, unit_uv(), argb(ABOUT_SIGNATURE));
        }
        let made = about.reset_made;
        ui.add_space(25.0);
        let (row, response) = ui.allocate_exact_size(vec2(width, 44.0), Sense::click());
        ui.painter().rect_filled(row, 22, argb(ABOUT_CARD));
        let at = pos2(row.left() + 15.0, row.center().y);
        text(
            ui.painter(),
            at,
            Align2::LEFT_CENTER,
            "Create new reset point",
            17.0,
            Weight::Regular,
            argb(ABOUT_ROW_INK),
        );
        let new_point = response.clicked();
        ui.add_space(11.0);
        let (row, response) =
            ui.allocate_exact_size(vec2(width, if made.is_some() { 69.0 } else { 44.0 }), Sense::click());
        ui.painter().rect_filled(row, 22, argb(ABOUT_CARD));
        if let Some(when) = made {
            let at = pos2(row.left() + 15.0, row.center().y - 10.5);
            text(ui.painter(), at, Align2::LEFT_CENTER, "Reset brush", 17.0, Weight::Regular, argb(ABOUT_ROW_INK));
            let tick = pos2(row.left() + 15.0 + 8.5, row.center().y + 13.0);
            ui.painter().circle_filled(tick, 8.5, argb(pc::ACCENT));
            look.icon(ui, &icons::MENU_TICK, tick, 20.0, Color32::WHITE);
            // the short date and time with a comma between
            let day = {
                use chrono::TimeZone;
                chrono::Local
                    .timestamp_opt(when, 0)
                    .single()
                    .map_or_else(String::new, |t| t.format("%-m/%-d/%y, %-I:%M %p").to_string())
            };
            let at = pos2(tick.x + 8.5 + 6.0, tick.y);
            text(
                ui.painter(),
                at,
                Align2::LEFT_CENTER,
                &format!("Last reset point: {day}"),
                13.0,
                Weight::Regular,
                argb(ABOUT_ROW_INK),
            );
        } else {
            let at = pos2(row.left() + 15.0, row.center().y);
            text(ui.painter(), at, Align2::LEFT_CENTER, "Reset brush", 17.0, Weight::Regular, argb(ABOUT_ROW_INK));
        }
        let reset = response.clicked();
        if new_point {
            session.studio_new_reset_point();
            self.about = Some(AboutShown::read(ui.ctx(), session));
        }
        if reset {
            session.studio_reset();
            self.read(ui.ctx(), session);
            self.sync_pad(session);
        }
    }
}

impl Studio {
    /// How tall a control is, `width` wide (for a card's height before it is drawn).
    fn control_height(&self, c: &Control, width: f32) -> f32 {
        if let (Control::Source(..), true) = (c, self.wide) {
            return 8.5 + 45.0 + 12.5 + width;
        }
        if let Control::About = c {
            let made = self.about.as_ref().is_some_and(|a| a.reset_made.is_some());
            return 295.0 + 25.0 + 44.0 + 11.0 + if made { 69.0 } else { 44.0 };
        }
        control_height(c, width)
    }
}

/// How tall a control is, `width` wide (for a card's height before it is drawn).
fn control_height(c: &Control, width: f32) -> f32 {
    match c {
        Control::Slider(..) => 55.0,
        Control::Toggle(..) | Control::Picker(..) => 47.0,
        Control::Choice(_, options) => 44.0 * options.len() as f32 + options.len().saturating_sub(1) as f32,
        Control::Kinds(..) => 34.0,
        Control::Source(..) => 60.0,
        Control::TaperBar(..) => 72.0 + 4.5 + 47.0,
        Control::PressureCurve => width - 16.0,
        Control::TiltAngle => (width - 10.0) * 221.0 / 271.5,
        Control::Roundness => width * 231.5 / 283.0,
        Control::Disclosure(..) => 60.0,
        Control::PreviewCard => width * 91.5 / 278.5,
        Control::About => 300.0,
    }
}

/// The brush's sample stroke for the Preview page's card.
fn brush_preview(ctx: &egui::Context, session: &CanvasSession) -> Option<TextureHandle> {
    let b = session.current_brush();
    let t = session.brush_preview(b.set, b.index, 1060, 324)?;
    let image = egui::ColorImage::from_rgba_premultiplied([t.width as usize, t.height as usize], &t.rgba);
    Some(ctx.load_texture("studio preview", image, egui::TextureOptions::LINEAR))
}

/// A blend mode's name, as the picker shows it (a file may give the grain a layer mode its list
/// leaves out).
fn blend_name(key: &str, value: f32) -> String {
    let modes =
        if key == "grain_blend" { procraft_core::grain_blend_modes() } else { procraft_core::brush_blend_modes() };
    let n = value.round() as u32;
    modes
        .into_iter()
        .chain(procraft_core::blend_modes())
        .find(|m| m.id == n)
        .map_or_else(|| "Normal".to_owned(), |m| m.name)
}

/// A studio slider in `row` (55 tall): its name and value (in a pill) on top, the bar under them;
/// dragged, the knob follows the drag as far as it travels. Returns the value set, while held.
#[allow(clippy::too_many_arguments)]
fn studio_slider(
    ui: &Ui,
    row: Rect,
    label: &str,
    value: f32,
    how: Shown,
    most: f32,
    enabled: bool,
    id: &str,
    typing: bool,
) -> SliderHit {
    let both = matches!(how, Shown::Both | Shown::Follow | Shown::Grade);
    // the name and value 2 below the top half's middle
    let line = Rect::from_min_size(pos2(row.left() + 10.0, row.top() + 2.0), vec2(row.width() - 13.5, 28.0));
    let shown = shown_value(value, how, most);
    let ink = if typing {
        Color32::WHITE
    } else if enabled {
        argb(LABEL)
    } else {
        argb(VALUE_OFF)
    };
    let galley = ui.painter().layout_no_wrap(shown, Weight::Regular.font(16.0), ink);
    let pill_w = (galley.size().x + 26.0).max(73.0);
    let pill = Rect::from_min_size(pos2(line.right() - pill_w, line.top()), vec2(pill_w, 28.0));
    let fill = if typing {
        argb(pc::ACCENT)
    } else if enabled {
        argb(PILL)
    } else {
        argb(PILL_OFF)
    };
    ui.painter().rect_filled(pill, 14, fill);
    ui.painter().galley(pill.center() - galley.size() / 2.0, galley, ink);
    // a tap on the value types it on a keypad beside it
    let tapped = enabled && ui.interact(pill, ui.scope_id().with(("pill", id)), Sense::click()).clicked();
    // the name shrinks to fit beside the pill, down to six tenths
    let room = pill.left() - 12.0 - line.left();
    let mut size = 17.0;
    while size > 17.0 * 0.6
        && ui.painter().layout_no_wrap(label.to_owned(), Weight::Regular.font(size), Color32::WHITE).size().x > room
    {
        size -= 0.5;
    }
    text(
        ui.painter(),
        pos2(line.left(), line.center().y),
        Align2::LEFT_CENTER,
        label,
        size,
        Weight::Regular,
        argb(LABEL),
    );
    let bar = Rect::from_min_max(
        pos2(row.left() + 10.0, row.top() + 2.0 + 28.0 + 3.5),
        pos2(row.right() - 10.5, row.top() + 2.0 + 28.0 + 23.5),
    );
    let y = bar.center().y;
    let t = (if both { (value + 1.0) / 2.0 } else { value }).clamp(0.0, 1.0);
    let x = bar.left() + KNOB_INSET + (bar.width() - 2.0 * KNOB_INSET) * t;
    if both {
        let mid = bar.center().x;
        ui.painter().line_segment([pos2(mid, y - 10.0), pos2(mid, y + 10.0)], Stroke::new(1.0, argb(TRACK)));
    }
    ui.painter().line_segment([pos2(bar.left(), y), pos2(bar.right(), y)], Stroke::new(3.0, argb(TRACK)));
    if enabled {
        let from = if both { bar.center().x } else { bar.left() };
        ui.painter().line_segment([pos2(from, y), pos2(x, y)], Stroke::new(3.0, argb(pc::ACCENT)));
    }
    let knob = Rect::from_center_size(pos2(x, y), vec2(16.0, 10.0));
    ui.painter().rect_filled(knob, 5, if enabled { Color32::WHITE } else { argb(KNOB_OFF) });
    let pill = tapped.then_some(pill);
    if !enabled {
        return SliderHit { set: None, pill };
    }
    let response = ui.interact(bar.expand2(vec2(0.0, 6.0)), ui.scope_id().with(("studio slider", id)), Sense::drag());
    let dx = response.drag_delta().x;
    if !response.dragged() || dx == 0.0 {
        return SliderHit { set: None, pill };
    }
    // relative: the knob follows the drag, as far as it travels
    let span = if both { 2.0 } else { 1.0 };
    let travel = (bar.width() - 2.0 * KNOB_INSET).max(1.0);
    let set = Some((value + dx / travel * span).clamp(if both { -1.0 } else { 0.0 }, 1.0));
    SliderHit { set, pill }
}

/// What a slider did: the value set (while held), and its pill tapped (where it is).
struct SliderHit {
    set: Option<f32>,
    pill: Option<Rect>,
}

/// The taper bars' switch linking their ends.
static LINK_TIPS: [Control; 2] = [
    Control::Toggle("pen_taper_link", "Link tip sizes", None),
    Control::Toggle("touch_taper_link", "Link tip sizes", None),
];

/// How far in from the row's ends the taper handles stop: where the sliders' knobs do.
const TAPER_TRAVEL: f32 = 15.0;
const TAPER_BOX: Themed = Themed(0xFF212121, 0xFFEBEBEB);
const TAPER_BAR: Themed = Themed(0xFF363636, 0xFFDCDCDC);
const TILT_BOX: Themed = Themed(0xFF1B1B1B, 0xFFFFFFFF);
const TILT_LABEL: Themed = Themed(0xFF8E8E8E, 0xFF808080);

/// How the tilt angle's graph is coloured: its box (rounded, or square in a keypad), the quarter
/// disc, the arc, the line, the angle's label, and its handle's middle while the tilt is off.
struct TiltLook {
    box_fill: u32,
    rounded: bool,
    quarter: u32,
    arc: u32,
    line: u32,
    label: u32,
    off: u32,
}

const DARK_TILT: TiltLook = TiltLook {
    box_fill: 0xFF1B1B1B,
    rounded: true,
    quarter: 0xFF262626,
    arc: 0xFF4A4A4A,
    line: 0xFF6E6E6E,
    label: 0xFF8E8E8E,
    off: 0xFF6E6E6E,
};

/// In the keypads: the disc darker than the box round it.
const LIGHT_TILT: TiltLook = TiltLook {
    box_fill: 0xFFE6E6E6,
    rounded: false,
    quarter: 0xFFDADADA,
    arc: 0xFFABABAB,
    line: 0xFF797979,
    label: 0xFFAAAAAA,
    off: 0xFFAAAAAA,
};
const ROUND_GREEN: u32 = 0xFF42E347;
const ROUND_CROSS: Themed = Themed(0xFF4C4C4C, 0xFFC0C0C0);
const ROUND_FILL: Themed = Themed(0xD6232323, 0xD6F0F0F0);
const ROUND_EDGE: Themed = Themed(0xFF565656, 0xFFC8C8C8);

/// The taper bar in `rect`: a box inset as far as the sliders' knobs travel, the bar across it
/// thinning to points at its ends as the start and end handles (on lines across the box) move in,
/// each up to half way. Returns the handle dragged (true: the start) and where to, while held.
fn taper_bar(ui: &Ui, rect: Rect, start: f32, end: f32, id: &str) -> Option<(bool, f32)> {
    let from = rect.left() + TAPER_TRAVEL;
    let to = rect.right() - TAPER_TRAVEL;
    let half = (to - from) / 2.0;
    let start_x = from + start * half;
    let end_x = to - end * half;
    let (left, right) = (from - 1.0, to + 1.0);
    let painter = ui.painter();
    painter.rect_filled(Rect::from_min_max(pos2(left, rect.top()), pos2(right, rect.bottom())), 5, argb(TAPER_BOX));
    let (y, thick) = (rect.center().y, 7.0);
    let bar = vec![
        pos2(left, y),
        pos2(start_x, y - thick),
        pos2(end_x, y - thick),
        pos2(right, y),
        pos2(end_x, y + thick),
        pos2(start_x, y + thick),
    ];
    painter.add(egui::Shape::convex_polygon(bar, argb(TAPER_BAR), Stroke::NONE));
    for x in [start_x, end_x] {
        painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(2.0, Color32::WHITE));
        painter.circle_filled(pos2(x, y), 6.5, Color32::WHITE);
        painter.circle_filled(pos2(x, y), 4.0, argb(pc::ACCENT));
    }
    let response = ui.interact(rect, ui.scope_id().with(("taper", id)), Sense::drag());
    let p = response.interact_pointer_pos().filter(|_| response.dragged())?;
    // the handle nearer where the drag began
    let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(p);
    let at_start = (origin.x - start_x).abs() <= (origin.x - end_x).abs();
    let v = if at_start { (p.x - from) / half } else { (to - p.x) / half };
    Some((at_start, v.clamp(0.0, 1.0)))
}

/// The tilt angle in `rect`: a quarter disc from the bottom-left corner with a line at the angle
/// (0 flat, 1 upright) and its handle on the arc; a drag anywhere turns it. Returns the angle set.
fn tilt_angle(ui: &Ui, rect: Rect, value: f32, look: &TiltLook, active: bool) -> Option<f32> {
    let painter = ui.painter();
    painter.rect_filled(rect, if look.rounded { 6.0 } else { 0.0 }, argb(look.box_fill));
    let corner = rect.left_bottom();
    let r = rect.height();
    let clipped = ui.painter_at(rect);
    clipped.circle_filled(corner, r, argb(look.quarter));
    clipped.circle_stroke(corner, r, Stroke::new(1.0, argb(look.arc)));
    let a = value.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2;
    let dir = vec2(a.cos(), -a.sin());
    // the line runs on to the box's edge; the handle sits on the arc
    let reach = (if dir.x > 1e-4 { rect.width() / dir.x } else { f32::MAX }).min(if dir.y < -1e-4 {
        rect.height() / -dir.y
    } else {
        f32::MAX
    });
    painter.line_segment([corner, corner + dir * reach], Stroke::new(1.0, argb(look.line)));
    painter.circle_filled(corner + dir * r, 5.5, Color32::WHITE);
    painter.circle_filled(corner + dir * r, 3.5, if active { argb(pc::ACCENT) } else { argb(look.off) });
    let degrees = (value.clamp(0.0, 1.0) * 90.0).round() as i32;
    let at = pos2(rect.right() - 9.0, rect.top() + 7.0);
    text(painter, at, Align2::RIGHT_TOP, &format!("{degrees}\u{b0}"), 17.0, Weight::Regular, argb(look.label));
    let response = ui.interact(rect, ui.scope_id().with(("tilt angle", look.rounded)), Sense::click_and_drag());
    let p = response.interact_pointer_pos().filter(|_| response.is_pointer_button_down_on() || response.clicked())?;
    // the angle of the press from the bottom-left corner, a quarter turn at most
    let angle = (corner.y - p.y).atan2(p.x - corner.x) / std::f32::consts::FRAC_PI_2;
    Some(angle.clamp(0.0, 1.0))
}

/// What the roundness graph changed.
enum Turned {
    Round(f32),
    Angle(f32),
}

/// The shape's roundness and angle in `rect`: an ellipse on crossed lines, its blue handles (the
/// ends of its short axis) dragged in and out for the roundness, its green handle (the end of its
/// long axis) dragged round for the angle, shown in degrees.
fn roundness_graph(ui: &Ui, rect: Rect, roundness: f32, angle: f32) -> Option<Turned> {
    let round = roundness.clamp(0.05, 1.0);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6, argb(TILT_BOX));
    let c = rect.center();
    let r = rect.height() * 78.0 / 231.5;
    // the crossed lines show dimly through the ellipse's fill
    painter.line_segment([pos2(rect.left(), c.y), pos2(rect.right(), c.y)], Stroke::new(1.0, argb(ROUND_CROSS)));
    painter.line_segment([pos2(c.x, rect.top()), pos2(c.x, rect.bottom())], Stroke::new(1.0, argb(ROUND_CROSS)));
    let a = angle.clamp(0.0, 1.0) * std::f32::consts::TAU;
    let long = vec2(a.cos(), -a.sin());
    let short = vec2(-long.y, long.x);
    let outline: Vec<Pos2> = (0..96)
        .map(|i| {
            let t = i as f32 / 96.0 * std::f32::consts::TAU;
            c + long * (r * t.cos()) + short * (r * round * t.sin())
        })
        .collect();
    painter.add(egui::Shape::convex_polygon(outline.clone(), argb(ROUND_FILL), Stroke::NONE));
    painter.add(egui::Shape::closed_line(outline, Stroke::new(1.0, argb(ROUND_EDGE))));
    for end in [c + short * (r * round), c - short * (r * round)] {
        painter.circle_filled(end, 5.6, Color32::WHITE);
        painter.circle_filled(end, 3.7, argb(pc::ACCENT));
    }
    painter.circle_filled(c + long * r, 5.6, Color32::WHITE);
    painter.circle_filled(c + long * r, 3.7, argb(ROUND_GREEN));
    let degrees = ((angle.clamp(0.0, 1.0) * 360.0).round() as i32) % 360;
    let at = pos2(rect.right() - 14.0, rect.top() + 12.0);
    text(&painter, at, Align2::RIGHT_TOP, &format!("{degrees}\u{b0}"), 17.0, Weight::Regular, argb(TILT_LABEL));
    let response = ui.interact(rect, ui.scope_id().with("roundness"), Sense::drag());
    let p = response.interact_pointer_pos().filter(|_| response.dragged())?;
    let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(p);
    // the green handle turns it; anything else squashes it
    let d = p - c;
    if (origin - (c + long * r)).length() < 28.0 {
        let t = (-d.y).atan2(d.x) / std::f32::consts::TAU;
        Some(Turned::Angle(if t < 0.0 { t + 1.0 } else { t }))
    } else {
        let along = (d.x * short.x + d.y * short.y).abs();
        Some(Turned::Round((along / r).clamp(0.05, 1.0)))
    }
}

const KEYPAD_FILL: u32 = 0xFFD5D5D5;
const KEYPAD_KEY: u32 = 0xFFE6E6E6;
const KEYPAD_INK: u32 = 0xFF1D1D1D;
const KEYPAD_TABS: u32 = 0xFFC1C1C1;
const KEYPAD_TAB_CHOSEN: u32 = 0xFF0A8CFD;
const KEYPAD_TAB_INK: u32 = 0xFF393939;
const KEYPAD_SWITCH_OFF: u32 = 0xFFC2C1C5;
const KEYPAD_WIDTH: f32 = 275.0;
/// The keypad's curve: a light grid, an edge, the curve in blue.
const KEYPAD_CURVE: crate::curve::CurveLook = crate::curve::CurveLook {
    fill: Themed(0xFFE2E2E2, 0xFFE2E2E2),
    grid: Themed(0xFFD0D0D0, 0xFFD0D0D0),
    edge: Themed(0xFF999999, 0xFF999999),
    corner: 0.0,
    line: Themed(pc::ACCENT, pc::ACCENT),
    line_width: 1.5,
    handle: (7.0, 5.0),
};

/// A slider keypad's pages: typing the value, the slider's own pressure curve, its own tilt.
#[derive(Clone, Copy, PartialEq, Eq)]
enum KeypadPage {
    Numeric,
    Pressure,
    Tilt,
}

/// A slider's keypad while it is open: the slider (its key, name, how it reads, the most a size
/// may be), its value's pill, the digits typed so far, the page shown, and what it offers besides
/// typing (its own pressure curve; its own tilt, with its switch and angle's keys).
struct Keypad {
    key: &'static str,
    label: &'static str,
    how: Shown,
    most: f32,
    pill: Rect,
    text: String,
    page: KeypadPage,
    pages: Vec<KeypadPage>,
    tilt_switch: String,
    tilt_angle: String,
    curve: Option<Vec<[f32; 2]>>,
    editor: crate::curve::CurveEditor,
    frame: u64,
}

/// How a slider's value is typed: the largest number, whether it may be negative, and what a
/// number makes of the value.
fn typed(how: Shown, most: f32) -> (i32, bool) {
    match how {
        Shown::Double | Shown::Twice => (200, false),
        Shown::Both | Shown::Follow | Shown::Grade => (100, true),
        Shown::Count => (16, false),
        Shown::Size => ((most * 100.0).round() as i32, false),
        _ => (100, false),
    }
}

fn typed_value(how: Shown, most: f32, n: i32) -> f32 {
    match how {
        Shown::Double | Shown::Twice => n as f32 / 200.0,
        Shown::Count => (n.max(1) - 1) as f32 / 15.0,
        Shown::Size => ((n as f32 / 100.0) / most.max(1e-3)).clamp(0.0, 1.0).powf(1.0 / 2.2),
        _ => n as f32 / 100.0,
    }
}

impl Keypad {
    fn open(
        ctx: &egui::Context,
        session: &CanvasSession,
        key: &'static str,
        label: &'static str,
        how: Shown,
        most: f32,
        pill: Rect,
    ) -> Keypad {
        let e = slider_extras(key.to_owned());
        let mut pages = vec![KeypadPage::Numeric];
        if e.pressure {
            pages.push(KeypadPage::Pressure);
        }
        if e.tilt {
            pages.push(KeypadPage::Tilt);
        }
        let points = session.studio_slider_curve(key.to_owned());
        let curve = (points.len() >= 4).then(|| points.as_chunks::<2>().0.to_vec());
        Keypad {
            key,
            label,
            how,
            most,
            pill,
            text: String::new(),
            page: KeypadPage::Numeric,
            pages,
            tilt_switch: e.tilt_switch,
            tilt_angle: e.tilt_angle,
            curve,
            editor: crate::curve::CurveEditor::default(),
            frame: ctx.cumulative_frame_nr(),
        }
    }

    /// The digits typed so far set the value at once (beyond the most, the most); a sign alone
    /// waits for its digits.
    fn kind(&mut self, next: String, studio: &mut Studio, session: &Arc<CanvasSession>) {
        if next == "-" {
            self.text = next;
            return;
        }
        let (most, _) = typed(self.how, self.most);
        let n = next.parse::<i32>().map_or(0, |n| n.clamp(-most, most));
        self.text = if next.is_empty() { String::new() } else { n.to_string() };
        studio.set(session, self.key, typed_value(self.how, self.most, n));
    }

    /// The keypad, a light popover to the right of the value's pill, its arrow at the value: the
    /// slider's name, its pages' tabs (when it has more than typing), then the page. False once
    /// closed (a press outside it).
    fn show(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &Arc<CanvasSession>,
        studio: &mut Studio,
    ) -> bool {
        let screen = ctx.content_rect();
        let tabs = self.pages.len();
        let height = match tabs {
            1 => 360.0,
            2 => 410.0,
            _ => 386.0,
        };
        let key_height = if tabs > 1 { 64.0 } else { 66.0 };
        let middle = self.pill.center().y;
        let top =
            (middle - height / 2.0).clamp(screen.top() + 2.0, (screen.bottom() - height - 2.0).max(screen.top() + 2.0));
        let arrow = crate::panel::ARROW_HEIGHT;
        // the arrow's tip a little into the pill, the body 3 past its end
        let rect = Rect::from_min_size(pos2(self.pill.right() - (arrow - 3.0), top), vec2(KEYPAD_WIDTH, height));
        let body = Rect::from_min_max(pos2(rect.left() + arrow, rect.top()), rect.max);
        let mut typed_key = None;
        egui::Area::new(Id::unique("slider keypad")).fixed_pos(rect.min).order(Order::Tooltip).show(ctx, |ui| {
            ui.allocate_rect(rect, Sense::click_and_drag());
            let painter = ui.painter();
            painter.rect_filled(body, 22, argb(KEYPAD_FILL));
            let tip = pos2(rect.left(), middle);
            let arrow_shape =
                vec![tip, pos2(body.left() + 0.5, middle - arrow), pos2(body.left() + 0.5, middle + arrow)];
            painter.add(egui::Shape::convex_polygon(arrow_shape, argb(KEYPAD_FILL), Stroke::NONE));
            let at = pos2(body.left() + 25.0, body.top() + 19.0);
            text(painter, at, Align2::LEFT_TOP, self.label, 22.0, Weight::Bold, argb(KEYPAD_INK));
            let mut y = body.top() + 19.0 + if tabs > 1 { 43.0 } else { 51.0 };
            if tabs > 1 {
                let row = Rect::from_min_size(pos2(body.left() + 12.0, y), vec2(251.0, 30.0));
                ui.painter().rect_filled(row, 9, argb(KEYPAD_TABS));
                let cell_w = (row.width() - 4.0) / tabs as f32;
                for (i, page) in self.pages.clone().into_iter().enumerate() {
                    let cell = Rect::from_min_size(
                        pos2(row.left() + 2.0 + cell_w * i as f32, row.top() + 2.0),
                        vec2(cell_w, 26.0),
                    );
                    let on = page == self.page;
                    if on {
                        ui.painter().rect_filled(cell, 7, argb(KEYPAD_TAB_CHOSEN));
                    }
                    let icon = match page {
                        KeypadPage::Numeric => &icons::KEYPAD,
                        KeypadPage::Pressure => &icons::PEN_PRESSURE,
                        KeypadPage::Tilt => &icons::PEN_TILT,
                    };
                    look.icon(ui, icon, cell.center(), 30.0, if on { Color32::WHITE } else { argb(KEYPAD_TAB_INK) });
                    if ui.interact(cell, ui.scope_id().with(("keypad page", i)), Sense::click()).clicked() {
                        self.page = page;
                    }
                }
                y = row.bottom() + 12.0;
            }
            match self.page {
                KeypadPage::Numeric => {
                    let (_, signed) = typed(self.how, self.most);
                    let left = body.left() + 12.0;
                    let rows: [&[(&str, f32)]; 4] = [
                        &[("7", 81.0), ("8", 81.0), ("9", 81.0)],
                        &[("4", 81.0), ("5", 81.0), ("6", 81.0)],
                        &[("1", 81.0), ("2", 81.0), ("3", 81.0)],
                        if signed { &[("-", 81.0), ("0", 81.0), ("<", 81.0)] } else { &[("0", 166.0), ("<", 81.0)] },
                    ];
                    for (r, row) in rows.iter().enumerate() {
                        let mut x = left;
                        for (label, w) in row.iter() {
                            let key =
                                Rect::from_min_size(pos2(x, y + (key_height + 5.0) * r as f32), vec2(*w, key_height));
                            ui.painter().rect_filled(key, 12, argb(KEYPAD_KEY));
                            if *label == "<" {
                                look.icon(ui, &icons::BACKSPACE, key.center(), 26.0, argb(KEYPAD_INK));
                            } else {
                                text(
                                    ui.painter(),
                                    key.center(),
                                    Align2::CENTER_CENTER,
                                    label,
                                    29.0,
                                    Weight::Regular,
                                    argb(KEYPAD_INK),
                                );
                            }
                            if ui.interact(key, ui.scope_id().with(("key", r, *label)), Sense::click()).clicked() {
                                typed_key = Some(*label);
                            }
                            x += w + 4.0;
                        }
                    }
                }
                KeypadPage::Pressure => {
                    y += 6.0;
                    let row = Rect::from_min_max(pos2(body.left() + 18.0, y), pos2(body.right() - 8.0, y + 28.0));
                    if switch_row(ui, row, "Pressure", self.curve.is_some()) {
                        self.curve = if self.curve.is_some() { None } else { Some(crate::curve::straight()) };
                        let points: Vec<f32> = self.curve.iter().flatten().flatten().copied().collect();
                        session.set_studio_slider_curve(self.key.to_owned(), points);
                        studio.changed(session);
                    }
                    let graph = Rect::from_min_size(pos2(body.left() + 18.0, row.bottom() + 12.0), vec2(239.0, 219.0));
                    let mut points = self.curve.clone().unwrap_or_default();
                    if self.editor.show(ui, graph, &mut points, &KEYPAD_CURVE, self.key) && self.curve.is_some() {
                        session
                            .set_studio_slider_curve(self.key.to_owned(), points.iter().flatten().copied().collect());
                        self.curve = Some(points);
                        studio.changed(session);
                    }
                }
                KeypadPage::Tilt => {
                    y += 6.0;
                    let on = (!self.tilt_switch.is_empty()).then(|| studio.value(&self.tilt_switch) >= 0.5);
                    if let Some(now) = on {
                        let row = Rect::from_min_max(pos2(body.left() + 18.0, y), pos2(body.right() - 8.0, y + 28.0));
                        if switch_row(ui, row, "Tilt", now) {
                            let key = self.tilt_switch.clone();
                            studio.set(session, &key, if now { 0.0 } else { 1.0 });
                        }
                        y = row.bottom();
                    }
                    let graph = Rect::from_min_size(pos2(body.left() + 10.0, y + 8.0), vec2(255.0, 209.0)).shrink(5.0);
                    let angle_key = self.tilt_angle.clone();
                    if let Some(v) = tilt_angle(ui, graph, studio.value(&angle_key), &LIGHT_TILT, on.unwrap_or(true)) {
                        studio.set(session, &angle_key, v);
                    }
                }
            }
        });
        if let Some(label) = typed_key {
            let next = match label {
                "<" => {
                    let mut t = self.text.clone();
                    t.pop();
                    t
                }
                "-" => {
                    if let Some(rest) = self.text.strip_prefix('-') {
                        rest.to_owned()
                    } else {
                        format!("-{}", self.text)
                    }
                }
                digit => format!("{}{digit}", self.text),
            };
            self.kind(next, studio, session);
        }
        let mut points = self.curve.clone().unwrap_or_default();
        if self.editor.menu(ctx, &mut points) && self.curve.is_some() {
            session.set_studio_slider_curve(self.key.to_owned(), points.iter().flatten().copied().collect());
            self.curve = Some(points);
            studio.changed(session);
        }
        // a press outside it (and its node menu) closes it
        let outside =
            ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p)));
        !(outside && !self.editor.menu_open() && ctx.cumulative_frame_nr() != self.frame)
    }
}

/// A page's name and its switch, in a row across the keypad; true when tapped.
fn switch_row(ui: &Ui, row: Rect, label: &str, on: bool) -> bool {
    text(
        ui.painter(),
        pos2(row.left(), row.center().y),
        Align2::LEFT_CENTER,
        label,
        17.0,
        Weight::Regular,
        argb(KEYPAD_INK),
    );
    let at = pos2(row.right() - 63.0, row.center().y - 14.0);
    let track = Rect::from_min_size(at, vec2(63.0, 28.0));
    ui.painter().rect_filled(track, 14, if on { argb(pc::ACCENT) } else { argb(KEYPAD_SWITCH_OFF) });
    let knob = Rect::from_min_size(at + vec2(2.0 + if on { 22.0 } else { 0.0 }, 2.0), vec2(37.0, 24.0));
    ui.painter().rect_filled(knob, 12, Color32::WHITE);
    ui.interact(row, ui.scope_id().with(("switch row", label)), Sense::click()).clicked()
}

const MODE_ROW: f32 = 38.0;
const MODE_LIST_WIDTH: f32 = 250.0;
const MODE_ROWS_SHOWN: usize = 11;
const MODE_LIST_FILL: u32 = 0xFFE1E1E1;
const MODE_LIST_LINE: u32 = 0xFFCDCDCD;
const MODE_LIST_INK: u32 = 0xFF393939;
const MODE_LIST_GLYPH: u32 = 0xFF7F7F7F;

impl Studio {
    /// Something changed the brush outside a control's own value: the sample stroke and the pad
    /// follow it.
    fn changed(&mut self, session: &Arc<CanvasSession>) {
        self.preview_due = Some(Instant::now() + PREVIEW_DELAY);
        self.sync_pad(session);
    }

    /// A blend mode's list, while open: the modes in the studio's order as a light list 10 from
    /// the window's right, the current one over its row (11 rows show, the rest scroll); a tap
    /// picks one, a tap outside closes it.
    fn blend_list_view(&mut self, ctx: &egui::Context, look: &mut Look, session: &Arc<CanvasSession>) {
        let Some((key, centre, fresh)) = self.blend_list else { return };
        let modes =
            if key == "grain_blend" { procraft_core::grain_blend_modes() } else { procraft_core::brush_blend_modes() };
        let current = self.value(key).round() as u32;
        let chosen = modes.iter().position(|m| m.id == current).unwrap_or(0);
        let shown = MODE_ROWS_SHOWN.min(modes.len());
        let first = chosen.saturating_sub(shown / 2).min(modes.len().saturating_sub(shown));
        let pitch = MODE_ROW + 1.0;
        let height = MODE_ROW * shown as f32 + (shown.saturating_sub(1)) as f32;
        let screen = ctx.content_rect();
        let top = (centre - (chosen - first) as f32 * pitch - pitch / 2.0)
            .clamp(screen.top() + 10.0, (screen.bottom() - 10.0 - height).max(screen.top() + 10.0));
        let rect =
            Rect::from_min_size(pos2(screen.right() - 10.0 - MODE_LIST_WIDTH, top), vec2(MODE_LIST_WIDTH, height));
        let mut picked = None;
        egui::Area::new(Id::unique("blend list")).fixed_pos(rect.min).order(Order::Tooltip).show(ctx, |ui| {
            let shadow =
                egui::epaint::Shadow { offset: [0, 6], blur: 24, spread: 0, color: Color32::from_black_alpha(0x60) };
            ui.painter().add(shadow.as_shape(rect, 13.0));
            ui.painter().rect_filled(rect, 13, argb(MODE_LIST_FILL));
            ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                let mut area = egui::ScrollArea::vertical()
                    .id_salt("blend list")
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
                if fresh {
                    area = area.vertical_scroll_offset(first as f32 * pitch);
                }
                area.show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    for (i, m) in modes.iter().enumerate() {
                        if i > 0 {
                            let (line, _) = ui.allocate_exact_size(vec2(MODE_LIST_WIDTH, 1.0), Sense::hover());
                            ui.painter().rect_filled(line, 0, argb(MODE_LIST_LINE));
                        }
                        let (row, response) = ui.allocate_exact_size(vec2(MODE_LIST_WIDTH, MODE_ROW), Sense::click());
                        let on = i == chosen;
                        if on {
                            ui.painter().rect_filled(row, 0, argb(pc::ACCENT));
                        }
                        let ink = if on { Color32::WHITE } else { argb(MODE_LIST_INK) };
                        let at = pos2(row.left() + 12.0, row.center().y);
                        text(ui.painter(), at, Align2::LEFT_CENTER, &m.name, 14.5, Weight::Regular, ink);
                        if let Some((_, glyph)) = icons::BLEND_GLYPHS.iter().find(|(name, _)| *name == m.name) {
                            let tint = if on { Color32::WHITE } else { argb(MODE_LIST_GLYPH) };
                            // its 26 frame centred 31 in from the right
                            look.icon(ui, glyph, pos2(row.right() - 31.0, row.center().y), 26.0, tint);
                        }
                        if response.clicked() {
                            picked = Some(m.id);
                        }
                    }
                });
            });
        });
        if let Some(id) = picked {
            self.set(session, key, id as f32);
            self.blend_list = None;
            return;
        }
        let outside =
            ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p)));
        self.blend_list = if outside && !fresh { None } else { Some((key, centre, false)) };
    }
}

const ABOUT_CARD: Themed = Themed(0xFF252525, 0xFFEBEBEB);
const ABOUT_FACE: Themed = Themed(0xFF3A3A3A, 0xFFD8D8D8);
const ABOUT_NAME: Themed = Themed(0xFFE0E0E0, 0xFF2A2A2A);
const ABOUT_ROW_INK: Themed = Themed(0xFFBBBBBB, 0xFF4A4A4A);
const ABOUT_SIGNATURE: Themed = Themed(0xFF595959, 0xFFA8A8A8);

/// The whole of a picture.
fn unit_uv() -> Rect {
    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))
}

/// Who made the studio's brush (their picture and signature as pictures) and when its reset point
/// was made.
struct AboutShown {
    author: String,
    picture: Option<TextureHandle>,
    signature: Option<TextureHandle>,
    reset_made: Option<i64>,
}

impl AboutShown {
    fn read(ctx: &egui::Context, session: &CanvasSession) -> AboutShown {
        let a = session.studio_about();
        let picture = |png: Option<Vec<u8>>, name: &str| {
            let rgba = image::load_from_memory(&png?).ok()?.to_rgba8();
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [rgba.width() as usize, rgba.height() as usize],
                rgba.as_raw(),
            );
            Some(ctx.load_texture(name, image, egui::TextureOptions::LINEAR))
        };
        AboutShown {
            author: a.author,
            picture: picture(a.picture, "brush maker"),
            signature: picture(a.signature, "brush signature"),
            reset_made: a.reset_made,
        }
    }
}

/// `texture` cropped to a circle of radius `r` about `centre` (filling it).
fn circle_image(ui: &Ui, texture: TextureId, centre: Pos2, r: f32) {
    let mut mesh = egui::Mesh::with_texture(texture);
    mesh.vertices.push(egui::epaint::Vertex { pos: centre, uv: pos2(0.5, 0.5), color: Color32::WHITE });
    let n = 64u32;
    for i in 0..=n {
        let t = i as f32 / n as f32 * std::f32::consts::TAU;
        let (sin, cos) = t.sin_cos();
        mesh.vertices.push(egui::epaint::Vertex {
            pos: centre + vec2(cos, sin) * r,
            uv: pos2(0.5 + cos * 0.5, 0.5 + sin * 0.5),
            color: Color32::WHITE,
        });
        if i > 0 {
            mesh.add_triangle(0, i, i + 1);
        }
    }
    ui.painter().add(mesh);
}
