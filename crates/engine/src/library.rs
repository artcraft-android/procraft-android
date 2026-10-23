//! ProCraft's own brushes, grouped in sets. Every preset here is designed for ProCraft on the
//! shared brush model (shapes and grains from `textures`).

use crate::brush::{Brush, Grain, GrainMode, Rendering, Shape, Taper, Tilt};
use crate::curve::Curve;
use crate::geom::Vec2;
use crate::textures::{grain, shape};

#[derive(Clone, Debug, PartialEq)]
pub struct BrushSet {
    pub name: &'static str,
    pub brushes: Vec<Brush>,
}

/// The brush sets. A set's place here and a brush's place in its set are how kept libraries refer
/// to what they started from, so new sets and brushes only ever go at the end; [`SHELF`] is the
/// order a new library shows them in.
pub fn library() -> Vec<BrushSet> {
    vec![
        BrushSet {
            name: "Sketching",
            brushes: vec![
                graphite(),
                soft_pencil(),
                technical_pencil(),
                lead_holder(),
                coloured_pencil(),
                sketch_pen(),
                hb_pencil(),
                six_b_pencil(),
                graphite_stick(),
                charcoal_pencil(),
                shading_pencil(),
                ballpoint(),
            ],
        },
        BrushSet {
            name: "Inking",
            brushes: vec![
                Brush::round_pen(),
                fine_liner(),
                brush_pen(),
                dip_pen(),
                marker(),
                dry_ink(),
                studio_ink(),
                gel_pen(),
                thick_liner(),
                felt_tip(),
                rough_ink(),
                ink_bleed(),
            ],
        },
        BrushSet {
            name: "Painting",
            brushes: vec![
                round_brush(),
                flat_brush(),
                bristle_brush(),
                dry_brush(),
                gouache(),
                oil_paint(),
                palette_knife(),
                filbert(),
                fan_brush(),
                rigger(),
                sable(),
                impasto(),
                acrylic(),
                tempera(),
                scumble(),
            ],
        },
        BrushSet {
            name: "Airbrushing",
            brushes: vec![
                soft_airbrush(),
                hard_airbrush(),
                medium_airbrush(),
                fine_mist(),
                flat_airbrush(),
                grainy_airbrush(),
                stipple_airbrush(),
                feather_airbrush(),
                tight_airbrush(),
            ],
        },
        BrushSet {
            name: "Charcoals",
            brushes: vec![
                charcoal_stick(),
                willow_charcoal(),
                compressed_charcoal(),
                vine_charcoal(),
                charcoal_dust(),
                charcoal_block(),
                burnt_stick(),
                smudged_charcoal(),
                soft_charcoal(),
            ],
        },
        BrushSet {
            name: "Textures",
            brushes: vec![
                speckles(),
                canvas_texture(),
                dot_screen(),
                crosshatch(),
                rough_wall(),
                grit(),
                stipple(),
                wood_grain(),
                brickwork(),
                fish_scales(),
                cell_wall(),
                marble(),
                pinstripe(),
                honeycomb(),
                leather(),
                sandpaper(),
                checkerboard(),
            ],
        },
        BrushSet {
            name: "Drawing",
            brushes: vec![
                chalk(),
                soft_pastel(),
                oil_pastel(),
                wax_crayon(),
                conte(),
                hard_pastel(),
                chalk_pencil(),
                pastel_pencil(),
                litho_crayon(),
            ],
        },
        BrushSet {
            name: "Watercolour",
            brushes: vec![
                wet_wash(),
                round_wash(),
                granulating_wash(),
                blot(),
                bleeding_wash(),
                flat_wash(),
                dry_wash(),
                salt(),
                splash(),
                mop_brush(),
            ],
        },
        BrushSet {
            name: "Calligraphy",
            brushes: vec![
                chisel_nib(),
                script_brush(),
                monoline(),
                ribbon(),
                pointed_nib(),
                broad_pen(),
                italic_nib(),
                felt_brush(),
                copperplate(),
            ],
        },
        BrushSet {
            name: "Spray Paint",
            brushes: vec![spray_can(), fat_cap(), flick(), mist(), drips(), hard_spray(), splatter_can(), skinny_cap()],
        },
        BrushSet {
            name: "Nature",
            brushes: vec![
                leaves(),
                grass(),
                clouds(),
                bark(),
                foliage(),
                pine(),
                twigs(),
                moss(),
                flowers(),
                fur(),
                pebbles(),
                feathers(),
                reeds(),
            ],
        },
        BrushSet {
            name: "Effects",
            brushes: vec![
                sparkle(),
                bokeh(),
                smoke(),
                snow(),
                light_trail(),
                glow(),
                stars(),
                bubbles(),
                embers(),
                rain(),
                snowflakes(),
            ],
        },
        BrushSet {
            name: "Pixel",
            brushes: vec![
                pixel(),
                dither(),
                hatching_pen(),
                pixel_checker(),
                pixel_spray(),
                pixel_round(),
                pixel_line(),
            ],
        },
        BrushSet {
            name: "Elements",
            brushes: vec![fire(), smoke_puff(), water_drops(), ripples(), lightning(), ice(), sea_foam(), fog()],
        },
        BrushSet {
            name: "Industrial",
            brushes: vec![
                rusted(),
                cement(),
                grunge(),
                scratched_metal(),
                steel_wool(),
                rivets(),
                grime(),
                oil_stain(),
            ],
        },
        BrushSet {
            name: "Organic",
            brushes: vec![hair(), skin_pores(), freckles(), reptile(), coral(), lichen(), seeds(), veins()],
        },
        BrushSet {
            name: "Abstract",
            brushes: vec![
                hearts(),
                confetti(),
                triangles(),
                diamonds(),
                speed_lines(),
                scribbles(),
                dot_trail(),
                dashes(),
            ],
        },
        BrushSet {
            name: "Touchups",
            brushes: vec![soft_blender(), grainy_blender(), retouch(), highlight(), shadow(), noise(), fine_detail()],
        },
        BrushSet {
            name: "Retro",
            brushes: vec![
                ben_day(),
                screen_tone(),
                comic_ink(),
                retro_stipple(),
                pop_dots(),
                retro_lines(),
                offset_print(),
            ],
        },
        BrushSet {
            name: "Vintage",
            brushes: vec![
                worn_ink(),
                letterpress(),
                old_paper(),
                sepia_wash(),
                scratchy_pen(),
                rubber_stamp(),
                faded_print(),
            ],
        },
        BrushSet {
            name: "Luminance",
            brushes: vec![
                neon(),
                glimmer(),
                lens_dust(),
                flare(),
                halo(),
                light_leak(),
                aurora(),
                fairy_dust(),
                soft_glow(),
                overlay_light(),
            ],
        },
    ]
}

/// The order a new library shows the sets in, by their place in [`library`].
pub const SHELF: [usize; 21] = [0, 1, 6, 2, 7, 8, 3, 9, 4, 5, 16, 13, 19, 20, 14, 15, 10, 11, 17, 18, 12];

fn named(name: &str, b: Brush) -> Brush {
    Brush { name: name.to_owned(), ..b }
}

/// `b` with Brush Studio controls set (keys and values as [`Brush::set_setting`] takes them).
fn with(mut b: Brush, settings: &[(&str, f32)]) -> Brush {
    for &(key, value) in settings {
        b.set_setting(key, value);
    }
    b
}

/// `b` shown at `preview` of its largest size in the library (scattered shapes need room).
fn previewed(b: Brush, preview: f32) -> Brush {
    with(b, &[("preview_size", preview)])
}

/// A grain fixed to the canvas.
fn paper(image: u32, scale: f32, depth: f32) -> Grain {
    Grain { image: Some(image), mode: GrainMode::Texturized, scale, depth, source: None }
}

/// A grain that travels with each stamp.
fn moving(image: u32, scale: f32, depth: f32) -> Grain {
    Grain { image: Some(image), mode: GrainMode::Moving, scale, depth, source: None }
}

/// A pressure curve that rises fast: a light touch already gives most of the line.
fn firm_curve() -> Curve {
    Curve::new(&[Vec2::new(0.0, 0.0), Vec2::new(0.35, 0.6), Vec2::new(1.0, 1.0)])
}

/// A pressure curve that eases in: light touches stay light.
fn soft_curve() -> Curve {
    Curve::new(&[Vec2::new(0.0, 0.0), Vec2::new(0.5, 0.3), Vec2::new(1.0, 1.0)])
}

fn graphite() -> Brush {
    named(
        "Graphite",
        Brush {
            spacing: 0.06,
            streamline: 0.1,
            hardness: 0.7,
            min_size: 1.0,
            max_size: 24.0,
            flow: 0.85,
            pressure_size: 0.3,
            pressure_opacity: 0.9,
            pressure_curve: soft_curve(),
            pen_taper: Taper { start: 0.05, end: 0.08, size: 0.4, opacity: 0.3 },
            touch_taper: Taper::NONE,
            shape: Shape { image: Some(shape::GRAINY_DISC), randomise: true, ..Shape::default() },
            grain: Grain {
                image: Some(grain::PAPER),
                mode: GrainMode::Texturized,
                scale: 120.0,
                depth: 0.6,
                source: None,
            },
            tilt: Tilt { angle: 0.35, size: 1.2, opacity: 0.55 },
            ..Brush::round_pen()
        },
    )
}

fn soft_pencil() -> Brush {
    named(
        "Soft Pencil",
        Brush {
            max_size: 40.0,
            flow: 0.7,
            grain: Grain {
                image: Some(grain::COARSE),
                mode: GrainMode::Texturized,
                scale: 200.0,
                depth: 0.7,
                source: None,
            },
            ..graphite()
        },
    )
}

fn technical_pencil() -> Brush {
    named(
        "Technical Pencil",
        Brush {
            spacing: 0.05,
            hardness: 0.95,
            max_size: 10.0,
            flow: 1.0,
            pressure_size: 0.1,
            pressure_opacity: 0.6,
            shape: Shape::default(),
            grain: Grain {
                image: Some(grain::PAPER),
                mode: GrainMode::Texturized,
                scale: 120.0,
                depth: 0.35,
                source: None,
            },
            tilt: Tilt::default(),
            ..graphite()
        },
    )
}

fn fine_liner() -> Brush {
    named(
        "Fine Liner",
        Brush {
            streamline: 0.5,
            hardness: 0.97,
            max_size: 30.0,
            pressure_size: 0.15,
            pen_taper: Taper::NONE,
            touch_taper: Taper::NONE,
            ..Brush::round_pen()
        },
    )
}

fn brush_pen() -> Brush {
    named(
        "Brush Pen",
        Brush {
            streamline: 0.45,
            max_size: 80.0,
            pressure_size: 0.95,
            pressure_curve: soft_curve(),
            pen_taper: Taper { start: 0.3, end: 0.4, size: 0.95, opacity: 0.0 },
            touch_taper: Taper { start: 0.2, end: 0.3, size: 0.9, opacity: 0.0 },
            ..Brush::round_pen()
        },
    )
}

fn round_brush() -> Brush {
    named(
        "Round Brush",
        Brush {
            spacing: 0.05,
            streamline: 0.2,
            hardness: 0.6,
            max_size: 160.0,
            flow: 0.35,
            pressure_size: 0.6,
            pressure_opacity: 0.5,
            pen_taper: Taper { start: 0.1, end: 0.1, size: 0.5, opacity: 0.2 },
            ..Brush::round_pen()
        },
    )
}

fn flat_brush() -> Brush {
    named(
        "Flat Brush",
        Brush {
            spacing: 0.03,
            streamline: 0.2,
            max_size: 160.0,
            flow: 0.9,
            pressure_size: 0.4,
            pressure_opacity: 0.4,
            shape: Shape {
                image: Some(shape::FLAT),
                follow: 1.0,
                rotation: std::f32::consts::FRAC_PI_2,
                roundness: 1.0,
                ..Shape::default()
            },
            grain: Grain { image: Some(grain::WEAVE), mode: GrainMode::Moving, scale: 90.0, depth: 0.35, source: None },
            // aligned stamps that don't build up keep the shape's streaks
            rendering: Rendering::Strongest,
            pen_taper: Taper::NONE,
            touch_taper: Taper::NONE,
            ..Brush::round_pen()
        },
    )
}

fn bristle_brush() -> Brush {
    named(
        "Bristle Brush",
        Brush {
            shape: Shape {
                image: Some(shape::BRISTLES),
                follow: 1.0,
                rotation: std::f32::consts::FRAC_PI_2,
                ..Shape::default()
            },
            grain: Grain { image: Some(grain::FIBRE), mode: GrainMode::Moving, scale: 120.0, depth: 0.6, source: None },
            flow: 0.9,
            ..flat_brush()
        },
    )
}

fn soft_airbrush() -> Brush {
    named(
        "Soft Airbrush",
        Brush {
            spacing: 0.04,
            streamline: 0.0,
            hardness: 0.0,
            min_size: 4.0,
            max_size: 600.0,
            flow: 0.08,
            pressure_size: 0.0,
            pressure_opacity: 0.8,
            pen_taper: Taper::NONE,
            touch_taper: Taper::NONE,
            shape: Shape { image: Some(shape::SOFT_ROUND), ..Shape::default() },
            ..Brush::round_pen()
        },
    )
}

fn hard_airbrush() -> Brush {
    named(
        "Hard Airbrush",
        Brush { hardness: 0.85, flow: 0.25, shape: Shape::default(), max_size: 400.0, ..soft_airbrush() },
    )
}

fn charcoal_stick() -> Brush {
    named(
        "Charcoal Stick",
        Brush {
            spacing: 0.08,
            streamline: 0.1,
            max_size: 90.0,
            flow: 0.6,
            pressure_size: 0.4,
            pressure_opacity: 0.7,
            shape: Shape {
                image: Some(shape::CHUNK),
                randomise: true,
                size_jitter: 0.25,
                opacity_jitter: 0.3,
                ..Shape::default()
            },
            grain: Grain {
                image: Some(grain::COARSE),
                mode: GrainMode::Texturized,
                scale: 260.0,
                depth: 0.95,
                source: None,
            },
            tilt: Tilt { angle: 0.3, size: 1.0, opacity: 0.3 },
            pen_taper: Taper { start: 0.05, end: 0.05, size: 0.3, opacity: 0.2 },
            touch_taper: Taper::NONE,
            ..Brush::round_pen()
        },
    )
}

fn willow_charcoal() -> Brush {
    previewed(
        named(
            "Willow Charcoal",
            Brush {
                max_size: 50.0,
                flow: 0.8,
                rendering: Rendering::Strongest,
                grain: Grain {
                    image: Some(grain::PAPER),
                    mode: GrainMode::Texturized,
                    scale: 200.0,
                    depth: 0.7,
                    source: None,
                },
                ..charcoal_stick()
            },
        ),
        0.3,
    )
}

fn speckles() -> Brush {
    previewed(
        named(
            "Speckles",
            Brush {
                spacing: 0.6,
                streamline: 0.0,
                max_size: 120.0,
                flow: 1.0,
                pressure_size: 0.3,
                pressure_opacity: 0.0,
                shape: Shape {
                    image: Some(shape::SPLATTER),
                    randomise: true,
                    throw: 0.6,
                    count: 2,
                    size_jitter: 0.6,
                    opacity_jitter: 0.4,
                    ..Shape::default()
                },
                pen_taper: Taper::NONE,
                touch_taper: Taper::NONE,
                ..Brush::round_pen()
            },
        ),
        0.35,
    )
}

fn canvas_texture() -> Brush {
    named(
        "Canvas Texture",
        Brush {
            spacing: 0.05,
            hardness: 0.3,
            max_size: 500.0,
            flow: 0.7,
            pressure_size: 0.0,
            pressure_opacity: 0.6,
            shape: Shape { image: Some(shape::SOFT_ROUND), ..Shape::default() },
            grain: Grain {
                image: Some(grain::WEAVE),
                mode: GrainMode::Texturized,
                scale: 64.0,
                depth: 1.0,
                source: None,
            },
            rendering: Rendering::Strongest,
            pen_taper: Taper::NONE,
            touch_taper: Taper::NONE,
            ..Brush::round_pen()
        },
    )
}

// ---- Sketching, from the lead holder on ----

fn lead_holder() -> Brush {
    named(
        "Lead Holder",
        Brush {
            hardness: 0.9,
            max_size: 8.0,
            flow: 0.9,
            pressure_size: 0.15,
            pressure_opacity: 0.85,
            shape: Shape::default(),
            grain: paper(grain::PAPER, 100.0, 0.25),
            tilt: Tilt { angle: 0.4, size: 0.6, opacity: 0.4 },
            ..graphite()
        },
    )
}

fn coloured_pencil() -> Brush {
    named(
        "Coloured Pencil",
        Brush {
            hardness: 0.8,
            max_size: 20.0,
            flow: 0.8,
            pressure_opacity: 0.95,
            grain: paper(grain::LINEN, 90.0, 0.65),
            ..graphite()
        },
    )
}

fn sketch_pen() -> Brush {
    named(
        "Sketch Pen",
        Brush {
            streamline: 0.25,
            hardness: 0.92,
            max_size: 14.0,
            pressure_size: 0.5,
            grain: paper(grain::PAPER, 160.0, 0.2),
            pen_taper: Taper { start: 0.04, end: 0.08, size: 0.6, opacity: 0.0 },
            ..Brush::round_pen()
        },
    )
}

// ---- Inking, from the dip pen on ----

fn dip_pen() -> Brush {
    named(
        "Dip Pen",
        Brush {
            streamline: 0.4,
            hardness: 0.98,
            min_size: 0.5,
            max_size: 40.0,
            pressure_size: 1.0,
            pressure_curve: soft_curve(),
            pen_taper: Taper { start: 0.15, end: 0.3, size: 1.0, opacity: 0.0 },
            touch_taper: Taper { start: 0.1, end: 0.2, size: 0.9, opacity: 0.0 },
            ..Brush::round_pen()
        },
    )
}

fn marker() -> Brush {
    named(
        "Marker",
        Brush {
            spacing: 0.03,
            streamline: 0.2,
            hardness: 0.9,
            max_size: 70.0,
            flow: 0.55,
            pressure_size: 0.0,
            pressure_opacity: 0.2,
            // a chisel tip held at one angle; overlaps in a stroke don't darken
            shape: Shape { roundness: 0.35, rotation: std::f32::consts::FRAC_PI_4, ..Shape::default() },
            rendering: Rendering::Strongest,
            pen_taper: Taper::NONE,
            touch_taper: Taper::NONE,
            ..Brush::round_pen()
        },
    )
}

fn dry_ink() -> Brush {
    named(
        "Dry Ink",
        Brush {
            streamline: 0.3,
            hardness: 0.95,
            max_size: 50.0,
            pressure_size: 0.7,
            grain: paper(grain::COARSE, 220.0, 0.55),
            pen_taper: Taper { start: 0.1, end: 0.2, size: 0.8, opacity: 0.0 },
            ..Brush::round_pen()
        },
    )
}

// ---- Painting, from the dry brush on ----

fn dry_brush() -> Brush {
    named(
        "Dry Brush",
        Brush {
            flow: 0.85,
            shape: Shape {
                image: Some(shape::DRY),
                follow: 1.0,
                rotation: std::f32::consts::FRAC_PI_2,
                ..Shape::default()
            },
            grain: moving(grain::FIBRE, 140.0, 0.5),
            ..flat_brush()
        },
    )
}

fn gouache() -> Brush {
    with(
        named(
            "Gouache",
            Brush {
                spacing: 0.04,
                streamline: 0.2,
                hardness: 0.75,
                max_size: 140.0,
                flow: 0.9,
                pressure_size: 0.5,
                pressure_opacity: 0.3,
                shape: Shape { image: Some(shape::CHUNK), randomise: true, ..Shape::default() },
                grain: moving(grain::PAPER, 160.0, 0.3),
                pen_taper: Taper::NONE,
                touch_taper: Taper::NONE,
                ..Brush::round_pen()
            },
        ),
        &[("wet_dilution", 0.05), ("wet_attack", 0.7), ("wet_pull", 0.25)],
    )
}

fn oil_paint() -> Brush {
    with(
        named("Oil Paint", Brush { flow: 1.0, max_size: 180.0, ..bristle_brush() }),
        &[("wet_dilution", 0.1), ("wet_attack", 0.35), ("wet_pull", 0.6), ("wet_grade", 0.3)],
    )
}

fn palette_knife() -> Brush {
    with(
        named(
            "Palette Knife",
            Brush {
                hardness: 1.0,
                flow: 1.0,
                pressure_size: 0.2,
                shape: Shape {
                    image: Some(shape::SQUARE),
                    follow: 1.0,
                    roundness: 0.25,
                    rotation: std::f32::consts::FRAC_PI_2,
                    ..Shape::default()
                },
                grain: moving(grain::CRUMPLED, 120.0, 0.35),
                ..flat_brush()
            },
        ),
        &[("wet_dilution", 0.1), ("wet_pull", 0.85), ("wet_attack", 0.8)],
    )
}

// ---- Airbrushing, from the medium airbrush on ----

fn medium_airbrush() -> Brush {
    named("Medium Airbrush", Brush { hardness: 0.5, flow: 0.15, shape: Shape::default(), ..soft_airbrush() })
}

fn fine_mist() -> Brush {
    named(
        "Fine Mist",
        Brush {
            spacing: 0.08,
            max_size: 300.0,
            flow: 0.5,
            shape: Shape { image: Some(shape::SPRAY), randomise: true, size_jitter: 0.2, ..Shape::default() },
            ..soft_airbrush()
        },
    )
}

fn flat_airbrush() -> Brush {
    named(
        "Flat Airbrush",
        Brush {
            flow: 0.25,
            shape: Shape {
                image: Some(shape::SOFT_ROUND),
                roundness: 0.3,
                follow: 1.0,
                rotation: std::f32::consts::FRAC_PI_2,
                ..Shape::default()
            },
            ..soft_airbrush()
        },
    )
}

// ---- Charcoals, from the compressed charcoal on ----

fn compressed_charcoal() -> Brush {
    named(
        "Compressed Charcoal",
        Brush { flow: 0.85, max_size: 70.0, grain: paper(grain::COARSE, 200.0, 0.8), ..charcoal_stick() },
    )
}

fn vine_charcoal() -> Brush {
    named(
        "Vine Charcoal",
        Brush {
            max_size: 22.0,
            flow: 0.55,
            shape: Shape { image: Some(shape::GRAINY_DISC), randomise: true, ..Shape::default() },
            grain: paper(grain::PAPER, 160.0, 0.85),
            ..charcoal_stick()
        },
    )
}

fn charcoal_dust() -> Brush {
    named(
        "Charcoal Dust",
        Brush {
            spacing: 0.1,
            hardness: 0.0,
            max_size: 260.0,
            flow: 0.18,
            pressure_size: 0.2,
            pressure_opacity: 0.8,
            shape: Shape { image: Some(shape::CLOUD), randomise: true, ..Shape::default() },
            grain: paper(grain::COARSE, 240.0, 0.9),
            pen_taper: Taper::NONE,
            ..charcoal_stick()
        },
    )
}

// ---- Textures, from the dot screen on ----

/// A broad soft brush laying `grain` down where it passes.
fn texture_brush(name: &str, image: u32, scale: f32) -> Brush {
    named(name, Brush { hardness: 0.6, flow: 0.9, grain: paper(image, scale, 1.0), ..canvas_texture() })
}

fn dot_screen() -> Brush {
    texture_brush("Dot Screen", grain::HALFTONE, 96.0)
}

fn crosshatch() -> Brush {
    texture_brush("Crosshatch", grain::HATCHING, 160.0)
}

fn rough_wall() -> Brush {
    texture_brush("Rough Wall", grain::CONCRETE, 256.0)
}

fn grit() -> Brush {
    texture_brush("Grit", grain::NOISE, 128.0)
}

fn stipple() -> Brush {
    texture_brush("Stipple", grain::STIPPLE, 200.0)
}

// ---- Drawing ----

fn chalk() -> Brush {
    named(
        "Chalk",
        Brush {
            spacing: 0.06,
            streamline: 0.1,
            max_size: 80.0,
            flow: 0.75,
            pressure_size: 0.3,
            pressure_opacity: 0.7,
            shape: Shape { image: Some(shape::PASTEL), randomise: true, ..Shape::default() },
            grain: paper(grain::CONCRETE, 220.0, 0.85),
            tilt: Tilt { angle: 0.3, size: 1.2, opacity: 0.3 },
            pen_taper: Taper { start: 0.03, end: 0.03, size: 0.3, opacity: 0.2 },
            touch_taper: Taper::NONE,
            ..Brush::round_pen()
        },
    )
}

fn soft_pastel() -> Brush {
    named(
        "Soft Pastel",
        Brush {
            max_size: 120.0,
            flow: 0.6,
            shape: Shape { image: Some(shape::PASTEL), randomise: true, size_jitter: 0.15, ..Shape::default() },
            grain: paper(grain::PAPER, 200.0, 0.75),
            ..chalk()
        },
    )
}

fn oil_pastel() -> Brush {
    named(
        "Oil Pastel",
        Brush {
            flow: 0.95,
            max_size: 90.0,
            grain: paper(grain::CRUMPLED, 180.0, 0.55),
            rendering: Rendering::Strongest,
            ..chalk()
        },
    )
}

fn wax_crayon() -> Brush {
    named(
        "Wax Crayon",
        Brush {
            hardness: 0.85,
            max_size: 40.0,
            flow: 0.9,
            shape: Shape::default(),
            grain: paper(grain::LINEN, 140.0, 0.9),
            ..chalk()
        },
    )
}

fn conte() -> Brush {
    named(
        "Conté",
        Brush {
            max_size: 50.0,
            flow: 0.8,
            shape: Shape { image: Some(shape::SQUARE), roundness: 0.5, follow_azimuth: true, ..Shape::default() },
            grain: paper(grain::PAPER, 150.0, 0.7),
            ..chalk()
        },
    )
}

// ---- Watercolour ----

fn wet_wash() -> Brush {
    with(
        named(
            "Wet Wash",
            Brush {
                spacing: 0.05,
                streamline: 0.15,
                hardness: 0.3,
                max_size: 300.0,
                flow: 0.85,
                pressure_size: 0.3,
                pressure_opacity: 0.6,
                shape: Shape { image: Some(shape::CLOUD), randomise: true, ..Shape::default() },
                grain: paper(grain::WASH, 400.0, 0.45),
                rendering: Rendering::Strongest,
                pen_taper: Taper::NONE,
                touch_taper: Taper::NONE,
                ..Brush::round_pen()
            },
        ),
        &[("wet_dilution", 0.55), ("wet_pull", 0.2), ("wet_edges", 0.55)],
    )
}

fn round_wash() -> Brush {
    with(
        named(
            "Round Wash",
            Brush {
                hardness: 0.6,
                max_size: 160.0,
                flow: 0.9,
                pressure_size: 0.8,
                shape: Shape::default(),
                pen_taper: Taper { start: 0.1, end: 0.2, size: 0.8, opacity: 0.0 },
                ..wet_wash()
            },
        ),
        &[("wet_dilution", 0.35), ("wet_edges", 0.4)],
    )
}

fn granulating_wash() -> Brush {
    with(
        named("Granulating Wash", Brush { grain: paper(grain::WASH, 220.0, 0.85), flow: 0.9, ..wet_wash() }),
        &[("burnt_edges", 0.3)],
    )
}

fn blot() -> Brush {
    with(
        named(
            "Blot",
            Brush {
                spacing: 0.9,
                max_size: 220.0,
                flow: 0.9,
                shape: Shape {
                    image: Some(shape::BLOT),
                    randomise: true,
                    throw: 0.4,
                    size_jitter: 0.5,
                    opacity_jitter: 0.3,
                    ..Shape::default()
                },
                ..wet_wash()
            },
        ),
        &[("wet_edges", 0.7), ("preview_size", 0.25)],
    )
}

fn bleeding_wash() -> Brush {
    with(named("Bleeding Wash", Brush { flow: 0.8, ..wet_wash() }), &[("wet_dilution", 0.8), ("wet_blur", 0.6)])
}

// ---- Calligraphy ----

/// A nib held at one angle: broad one way, thin the other.
fn nib(name: &str, roundness: f32, angle: f32) -> Brush {
    named(
        name,
        Brush {
            spacing: 0.02,
            streamline: 0.45,
            hardness: 1.0,
            max_size: 60.0,
            pressure_size: 0.35,
            pressure_opacity: 0.0,
            shape: Shape { roundness, rotation: angle, ..Shape::default() },
            pen_taper: Taper::NONE,
            touch_taper: Taper::NONE,
            ..Brush::round_pen()
        },
    )
}

fn chisel_nib() -> Brush {
    nib("Chisel Nib", 0.18, std::f32::consts::FRAC_PI_4)
}

fn ribbon() -> Brush {
    Brush { max_size: 90.0, pressure_size: 0.1, ..nib("Ribbon", 0.08, std::f32::consts::FRAC_PI_6) }
}

fn script_brush() -> Brush {
    named(
        "Script Brush",
        Brush {
            streamline: 0.55,
            max_size: 70.0,
            pressure_size: 1.0,
            pressure_curve: firm_curve(),
            pen_taper: Taper { start: 0.2, end: 0.45, size: 1.0, opacity: 0.0 },
            touch_taper: Taper { start: 0.25, end: 0.35, size: 0.9, opacity: 0.0 },
            ..brush_pen()
        },
    )
}

fn monoline() -> Brush {
    named(
        "Monoline",
        Brush {
            streamline: 0.65,
            hardness: 1.0,
            max_size: 30.0,
            pressure_size: 0.0,
            pressure_opacity: 0.0,
            pen_taper: Taper::NONE,
            touch_taper: Taper::NONE,
            ..Brush::round_pen()
        },
    )
}

fn pointed_nib() -> Brush {
    named(
        "Pointed Nib",
        Brush {
            streamline: 0.5,
            min_size: 0.3,
            max_size: 36.0,
            pressure_size: 1.0,
            pressure_curve: soft_curve(),
            pen_taper: Taper { start: 0.05, end: 0.1, size: 1.0, opacity: 0.0 },
            ..dip_pen()
        },
    )
}

// ---- Spray Paint ----

fn spray_can() -> Brush {
    named(
        "Spray Can",
        Brush {
            spacing: 0.12,
            streamline: 0.05,
            hardness: 0.5,
            max_size: 260.0,
            flow: 0.85,
            pressure_size: 0.4,
            pressure_opacity: 0.5,
            shape: Shape { image: Some(shape::SPRAY), randomise: true, throw: 0.05, ..Shape::default() },
            pen_taper: Taper::NONE,
            touch_taper: Taper::NONE,
            ..Brush::round_pen()
        },
    )
}

fn fat_cap() -> Brush {
    named(
        "Fat Cap",
        Brush {
            max_size: 500.0,
            flow: 0.7,
            shape: Shape { image: Some(shape::SPRAY), randomise: true, count: 2, throw: 0.1, ..Shape::default() },
            ..spray_can()
        },
    )
}

fn flick() -> Brush {
    named(
        "Flick",
        Brush {
            spacing: 1.2,
            max_size: 300.0,
            flow: 1.0,
            shape: Shape {
                image: Some(shape::SPLATTER),
                randomise: true,
                throw: 1.0,
                size_jitter: 0.7,
                opacity_jitter: 0.2,
                ..Shape::default()
            },
            ..spray_can()
        },
    )
}

fn mist() -> Brush {
    named(
        "Mist",
        Brush {
            spacing: 0.06,
            hardness: 0.0,
            max_size: 600.0,
            flow: 0.2,
            shape: Shape { image: Some(shape::SOFT_ROUND), ..Shape::default() },
            grain: paper(grain::NOISE, 96.0, 0.7),
            ..spray_can()
        },
    )
}

// ---- Nature ----

/// Colour that varies a little from stamp to stamp, as leaves and blades do.
const NATURAL: [(&str, f32); 2] = [("stamp_hue", 0.06), ("stamp_darkness", 0.3)];

fn leaves() -> Brush {
    previewed(
        with(
            named(
                "Leaves",
                Brush {
                    spacing: 0.7,
                    streamline: 0.0,
                    max_size: 160.0,
                    flow: 1.0,
                    pressure_size: 0.4,
                    pressure_opacity: 0.0,
                    shape: Shape {
                        image: Some(shape::LEAF),
                        randomise: true,
                        throw: 0.8,
                        size_jitter: 0.5,
                        ..Shape::default()
                    },
                    pen_taper: Taper::NONE,
                    touch_taper: Taper::NONE,
                    ..Brush::round_pen()
                },
            ),
            &NATURAL,
        ),
        0.3,
    )
}

fn grass() -> Brush {
    previewed(
        with(
            named(
                "Grass",
                Brush {
                    spacing: 0.35,
                    shape: Shape { image: Some(shape::GRASS), throw: 0.15, size_jitter: 0.4, ..Shape::default() },
                    ..leaves()
                },
            ),
            &NATURAL,
        ),
        0.3,
    )
}

fn clouds() -> Brush {
    named(
        "Clouds",
        Brush {
            spacing: 0.25,
            streamline: 0.0,
            hardness: 0.2,
            max_size: 500.0,
            flow: 0.35,
            pressure_size: 0.3,
            pressure_opacity: 0.6,
            shape: Shape {
                image: Some(shape::CLOUD),
                randomise: true,
                throw: 0.3,
                size_jitter: 0.4,
                ..Shape::default()
            },
            pen_taper: Taper::NONE,
            touch_taper: Taper::NONE,
            ..Brush::round_pen()
        },
    )
}

fn bark() -> Brush {
    named("Bark", Brush { grain: moving(grain::CRUMPLED, 100.0, 0.7), flow: 0.95, ..dry_brush() })
}

fn foliage() -> Brush {
    previewed(
        with(
            named(
                "Foliage",
                Brush {
                    spacing: 0.3,
                    shape: Shape {
                        image: Some(shape::CHUNK),
                        randomise: true,
                        throw: 0.6,
                        count: 2,
                        size_jitter: 0.5,
                        opacity_jitter: 0.3,
                        ..Shape::default()
                    },
                    grain: moving(grain::COARSE, 120.0, 0.6),
                    ..leaves()
                },
            ),
            &NATURAL,
        ),
        0.25,
    )
}

// ---- Effects ----

fn sparkle() -> Brush {
    previewed(
        named(
            "Sparkle",
            Brush {
                spacing: 1.2,
                streamline: 0.0,
                max_size: 200.0,
                flow: 1.0,
                pressure_size: 0.5,
                pressure_opacity: 0.0,
                shape: Shape { image: Some(shape::SPARKLE), throw: 1.2, size_jitter: 0.5, ..Shape::default() },
                pen_taper: Taper::NONE,
                touch_taper: Taper::NONE,
                ..Brush::round_pen()
            },
        ),
        0.25,
    )
}

fn bokeh() -> Brush {
    previewed(
        named(
            "Bokeh",
            Brush {
                spacing: 0.8,
                max_size: 260.0,
                flow: 0.9,
                shape: Shape {
                    image: Some(shape::RING),
                    throw: 1.4,
                    size_jitter: 0.6,
                    opacity_jitter: 0.4,
                    ..Shape::default()
                },
                ..sparkle()
            },
        ),
        0.2,
    )
}

fn smoke() -> Brush {
    named(
        "Smoke",
        Brush {
            spacing: 0.08,
            streamline: 0.3,
            hardness: 0.0,
            max_size: 400.0,
            flow: 0.22,
            pressure_size: 0.3,
            pressure_opacity: 0.6,
            shape: Shape { image: Some(shape::WISP), follow: 1.0, size_jitter: 0.2, ..Shape::default() },
            pen_taper: Taper::NONE,
            touch_taper: Taper::NONE,
            ..Brush::round_pen()
        },
    )
}

fn snow() -> Brush {
    previewed(
        named(
            "Snow",
            Brush {
                spacing: 1.0,
                hardness: 0.4,
                max_size: 60.0,
                shape: Shape { throw: 2.0, count: 3, size_jitter: 0.8, opacity_jitter: 0.5, ..Shape::default() },
                ..sparkle()
            },
        ),
        0.4,
    )
}

fn light_trail() -> Brush {
    with(
        named(
            "Light Trail",
            Brush {
                spacing: 0.03,
                streamline: 0.5,
                hardness: 0.0,
                max_size: 120.0,
                flow: 0.35,
                pressure_size: 0.6,
                pressure_opacity: 0.3,
                shape: Shape { image: Some(shape::SOFT_ROUND), ..Shape::default() },
                pen_taper: Taper { start: 0.2, end: 0.4, size: 0.9, opacity: 0.6 },
                touch_taper: Taper { start: 0.2, end: 0.4, size: 0.9, opacity: 0.6 },
                ..Brush::round_pen()
            },
        ),
        &[("blend_mode", ADD)],
    )
}

/// Soft light added to what is under it: it glows over dark colours (and shows nothing on white).
fn glow() -> Brush {
    with(
        named(
            "Glow",
            Brush {
                spacing: 0.05,
                streamline: 0.2,
                hardness: 0.0,
                max_size: 500.0,
                flow: 0.15,
                pressure_size: 0.4,
                pressure_opacity: 0.6,
                shape: Shape { image: Some(shape::SOFT_ROUND), ..Shape::default() },
                pen_taper: Taper::NONE,
                touch_taper: Taper::NONE,
                ..Brush::round_pen()
            },
        ),
        &[("blend_mode", ADD)],
    )
}

/// The Add blend mode's number.
const ADD: f32 = 9.0;

// ---- Pixel ----

fn pixel() -> Brush {
    named(
        "Pixel",
        Brush {
            spacing: 0.05,
            streamline: 0.0,
            hardness: 1.0,
            min_size: 1.0,
            max_size: 32.0,
            flow: 1.0,
            pressure_size: 0.0,
            pressure_opacity: 0.0,
            shape: Shape { image: Some(shape::SQUARE), ..Shape::default() },
            rendering: Rendering::Strongest,
            pen_taper: Taper::NONE,
            touch_taper: Taper::NONE,
            ..Brush::round_pen()
        },
    )
}

fn dither() -> Brush {
    previewed(named("Dither", Brush { max_size: 200.0, grain: paper(grain::HALFTONE, 64.0, 1.0), ..pixel() }), 0.2)
}

fn hatching_pen() -> Brush {
    previewed(
        named(
            "Hatching Pen",
            Brush {
                spacing: 0.15,
                max_size: 120.0,
                shape: Shape { image: Some(shape::HATCH), ..Shape::default() },
                rendering: Rendering::Strongest,
                ..pixel()
            },
        ),
        0.3,
    )
}

// ---- More brushes, set by set ----

/// The Multiply, Screen, Colour Dodge, Overlay and Soft Light blend modes' numbers.
const MULTIPLY: f32 = 1.0;
const SCREEN: f32 = 7.0;
const DODGE: f32 = 8.0;
const OVERLAY: f32 = 11.0;
const SOFT_LIGHT: f32 = 12.0;

/// A brush laying `image` down in separate stamps, `spacing` apart (of their size), turned at
/// random and thrown `throw` of their size from the line: no streamline, no tapers.
fn stamps(name: &str, image: u32, spacing: f32, max_size: f32, throw: f32) -> Brush {
    named(
        name,
        Brush {
            spacing,
            streamline: 0.0,
            max_size,
            flow: 1.0,
            pressure_size: 0.4,
            pressure_opacity: 0.0,
            shape: Shape { image: Some(image), randomise: true, throw, size_jitter: 0.4, ..Shape::default() },
            pen_taper: Taper::NONE,
            touch_taper: Taper::NONE,
            ..Brush::round_pen()
        },
    )
}

/// `image` lying along the stroke: what runs up and down the image runs along the line.
fn along(image: u32) -> Shape {
    Shape { image: Some(image), follow: 1.0, rotation: std::f32::consts::FRAC_PI_2, ..Shape::default() }
}

// Sketching

fn hb_pencil() -> Brush {
    named(
        "HB Pencil",
        Brush { hardness: 0.8, max_size: 14.0, flow: 0.75, grain: paper(grain::PAPER, 110.0, 0.5), ..graphite() },
    )
}

fn six_b_pencil() -> Brush {
    named(
        "6B Pencil",
        Brush {
            max_size: 48.0,
            flow: 0.95,
            pressure_opacity: 0.7,
            grain: paper(grain::COARSE, 160.0, 0.75),
            tilt: Tilt { angle: 0.3, size: 1.6, opacity: 0.6 },
            ..graphite()
        },
    )
}

fn graphite_stick() -> Brush {
    named(
        "Graphite Stick",
        Brush {
            max_size: 70.0,
            flow: 0.7,
            shape: Shape { image: Some(shape::PASTEL), randomise: true, ..Shape::default() },
            grain: paper(grain::PAPER, 140.0, 0.8),
            tilt: Tilt { angle: 0.25, size: 2.0, opacity: 0.4 },
            ..graphite()
        },
    )
}

fn charcoal_pencil() -> Brush {
    named(
        "Charcoal Pencil",
        Brush { hardness: 0.75, max_size: 26.0, flow: 0.9, grain: paper(grain::COARSE, 180.0, 0.85), ..graphite() },
    )
}

/// The side of the lead: broad, soft and built up with pressure.
fn shading_pencil() -> Brush {
    named(
        "Shading Pencil",
        Brush {
            spacing: 0.05,
            hardness: 0.4,
            max_size: 110.0,
            flow: 0.35,
            pressure_size: 0.1,
            pressure_opacity: 1.0,
            shape: Shape { image: Some(shape::SOFT_ROUND), ..Shape::default() },
            grain: paper(grain::PAPER, 120.0, 0.85),
            pen_taper: Taper::NONE,
            ..graphite()
        },
    )
}

fn ballpoint() -> Brush {
    named(
        "Ballpoint",
        Brush {
            streamline: 0.2,
            hardness: 0.95,
            max_size: 7.0,
            flow: 0.85,
            pressure_size: 0.2,
            pressure_opacity: 0.6,
            grain: paper(grain::PAPER, 90.0, 0.15),
            pen_taper: Taper { start: 0.02, end: 0.04, size: 0.3, opacity: 0.3 },
            ..Brush::round_pen()
        },
    )
}

// Inking

fn studio_ink() -> Brush {
    named(
        "Studio Ink",
        Brush {
            streamline: 0.6,
            hardness: 1.0,
            max_size: 60.0,
            pressure_size: 0.9,
            pressure_curve: firm_curve(),
            pen_taper: Taper { start: 0.2, end: 0.3, size: 1.0, opacity: 0.0 },
            ..Brush::round_pen()
        },
    )
}

fn gel_pen() -> Brush {
    named(
        "Gel Pen",
        Brush {
            streamline: 0.3,
            hardness: 0.97,
            max_size: 12.0,
            pressure_size: 0.2,
            pen_taper: Taper { start: 0.02, end: 0.03, size: 0.4, opacity: 0.0 },
            ..Brush::round_pen()
        },
    )
}

fn thick_liner() -> Brush {
    named("Thick Liner", Brush { max_size: 90.0, pressure_size: 0.25, ..fine_liner() })
}

fn felt_tip() -> Brush {
    named(
        "Felt Tip",
        Brush {
            spacing: 0.04,
            streamline: 0.25,
            hardness: 0.85,
            max_size: 24.0,
            flow: 0.75,
            pressure_size: 0.15,
            pressure_opacity: 0.25,
            grain: paper(grain::FIBRE, 120.0, 0.25),
            rendering: Rendering::Strongest,
            pen_taper: Taper::NONE,
            touch_taper: Taper::NONE,
            ..Brush::round_pen()
        },
    )
}

/// Ink with a ragged edge: each stamp thrown a little to one side.
fn rough_ink() -> Brush {
    with(
        named("Rough Ink", Brush { spacing: 0.03, streamline: 0.0, max_size: 30.0, pressure_size: 0.6, ..dip_pen() }),
        &[("jitter_lateral", 0.1)],
    )
}

fn ink_bleed() -> Brush {
    with(
        named(
            "Ink Bleed",
            Brush {
                hardness: 0.7,
                max_size: 50.0,
                pressure_size: 0.7,
                grain: paper(grain::WASH, 200.0, 0.3),
                ..dip_pen()
            },
        ),
        &[("wet_edges", 0.6), ("wet_dilution", 0.2)],
    )
}

// Painting

fn filbert() -> Brush {
    with(
        named(
            "Filbert",
            Brush {
                hardness: 0.7,
                shape: Shape { roundness: 0.6, ..along(shape::FLAT) },
                grain: moving(grain::FIBRE, 120.0, 0.4),
                ..flat_brush()
            },
        ),
        &[("wet_dilution", 0.1), ("wet_attack", 0.4), ("wet_pull", 0.4)],
    )
}

fn fan_brush() -> Brush {
    named(
        "Fan Brush",
        Brush {
            spacing: 0.02,
            flow: 0.9,
            max_size: 200.0,
            shape: along(shape::HAIR),
            grain: Grain::default(),
            rendering: Rendering::Strongest,
            ..flat_brush()
        },
    )
}

/// A long thin liner: hair-fine at both ends.
fn rigger() -> Brush {
    named(
        "Rigger",
        Brush {
            streamline: 0.4,
            hardness: 0.85,
            max_size: 16.0,
            flow: 0.9,
            pressure_size: 0.9,
            grain: moving(grain::FIBRE, 80.0, 0.2),
            pen_taper: Taper { start: 0.3, end: 0.5, size: 1.0, opacity: 0.0 },
            ..round_brush()
        },
    )
}

fn sable() -> Brush {
    with(
        named(
            "Sable",
            Brush {
                hardness: 0.75,
                max_size: 90.0,
                flow: 0.6,
                pressure_size: 0.8,
                pressure_opacity: 0.3,
                shape: Shape { image: Some(shape::SOFT_ROUND), ..Shape::default() },
                ..round_brush()
            },
        ),
        &[("wet_dilution", 0.1), ("wet_pull", 0.3)],
    )
}

fn impasto() -> Brush {
    with(
        named(
            "Impasto",
            Brush {
                hardness: 0.9,
                max_size: 120.0,
                flow: 1.0,
                shape: Shape { image: Some(shape::CHUNK), randomise: true, ..Shape::default() },
                grain: moving(grain::CRUMPLED, 100.0, 0.5),
                ..gouache()
            },
        ),
        &[("wet_dilution", 0.1), ("wet_attack", 0.9), ("wet_pull", 0.7)],
    )
}

fn acrylic() -> Brush {
    with(
        named("Acrylic", Brush { flow: 0.95, grain: moving(grain::WEAVE, 100.0, 0.45), ..bristle_brush() }),
        &[("wet_dilution", 0.1), ("wet_attack", 0.6), ("wet_pull", 0.35)],
    )
}

fn tempera() -> Brush {
    named(
        "Tempera",
        Brush {
            hardness: 0.85,
            max_size: 40.0,
            flow: 0.8,
            grain: paper(grain::PAPER, 120.0, 0.4),
            rendering: Rendering::Strongest,
            ..round_brush()
        },
    )
}

/// Thin broken colour dragged over what is there.
fn scumble() -> Brush {
    named(
        "Scumble",
        Brush {
            hardness: 0.5,
            max_size: 200.0,
            flow: 0.4,
            shape: Shape { image: Some(shape::CHUNK), randomise: true, throw: 0.15, ..Shape::default() },
            grain: paper(grain::COARSE, 200.0, 0.8),
            ..round_brush()
        },
    )
}

// Airbrushing

fn grainy_airbrush() -> Brush {
    named("Grainy Airbrush", Brush { flow: 0.15, grain: paper(grain::NOISE, 64.0, 0.8), ..soft_airbrush() })
}

fn stipple_airbrush() -> Brush {
    named("Stipple Airbrush", Brush { flow: 0.3, grain: paper(grain::SPECKLE, 128.0, 1.0), ..soft_airbrush() })
}

fn feather_airbrush() -> Brush {
    named(
        "Feather Airbrush",
        Brush {
            flow: 0.12,
            pressure_size: 0.6,
            pen_taper: Taper { start: 0.2, end: 0.3, size: 0.9, opacity: 0.5 },
            ..soft_airbrush()
        },
    )
}

fn tight_airbrush() -> Brush {
    named(
        "Tight Airbrush",
        Brush {
            hardness: 0.95,
            max_size: 200.0,
            flow: 0.5,
            pressure_size: 0.5,
            shape: Shape::default(),
            ..soft_airbrush()
        },
    )
}

// Charcoals

fn charcoal_block() -> Brush {
    named(
        "Charcoal Block",
        Brush {
            max_size: 140.0,
            flow: 0.75,
            // the side of the block, broad across the stroke
            shape: Shape { image: Some(shape::SQUARE), roundness: 0.35, ..along(shape::SQUARE) },
            grain: paper(grain::COARSE, 260.0, 0.95),
            ..charcoal_stick()
        },
    )
}

fn burnt_stick() -> Brush {
    named(
        "Burnt Stick",
        Brush {
            max_size: 60.0,
            flow: 0.9,
            shape: Shape { image: Some(shape::GRAINY_DISC), randomise: true, ..Shape::default() },
            grain: paper(grain::CRUMPLED, 220.0, 0.8),
            ..charcoal_stick()
        },
    )
}

fn smudged_charcoal() -> Brush {
    named(
        "Smudged Charcoal",
        Brush {
            hardness: 0.0,
            max_size: 200.0,
            flow: 0.3,
            shape: along(shape::SMUDGE),
            grain: paper(grain::PAPER, 200.0, 0.6),
            ..charcoal_stick()
        },
    )
}

fn soft_charcoal() -> Brush {
    named(
        "Soft Charcoal",
        Brush { max_size: 70.0, flow: 0.7, grain: paper(grain::SAND, 160.0, 0.9), ..charcoal_stick() },
    )
}

// Textures

fn wood_grain() -> Brush {
    texture_brush("Wood Grain", grain::WOOD, 400.0)
}

fn brickwork() -> Brush {
    texture_brush("Brickwork", grain::BRICK, 512.0)
}

fn fish_scales() -> Brush {
    texture_brush("Fish Scales", grain::SCALES, 256.0)
}

fn cell_wall() -> Brush {
    texture_brush("Cell Wall", grain::CELLS, 300.0)
}

fn marble() -> Brush {
    texture_brush("Marble", grain::MARBLE, 600.0)
}

fn pinstripe() -> Brush {
    texture_brush("Pinstripe", grain::STRIPES, 160.0)
}

fn honeycomb() -> Brush {
    texture_brush("Honeycomb", grain::HEXAGONS, 400.0)
}

fn leather() -> Brush {
    texture_brush("Leather", grain::LEATHER, 300.0)
}

fn sandpaper() -> Brush {
    texture_brush("Sandpaper", grain::SAND, 128.0)
}

fn checkerboard() -> Brush {
    texture_brush("Checkerboard", grain::CHECKER, 128.0)
}

// Drawing

fn hard_pastel() -> Brush {
    named(
        "Hard Pastel",
        Brush {
            max_size: 60.0,
            flow: 0.8,
            shape: Shape { image: Some(shape::SQUARE), roundness: 0.6, follow_azimuth: true, ..Shape::default() },
            grain: paper(grain::SAND, 180.0, 0.8),
            ..chalk()
        },
    )
}

fn chalk_pencil() -> Brush {
    named(
        "Chalk Pencil",
        Brush {
            hardness: 0.8,
            max_size: 18.0,
            flow: 0.85,
            shape: Shape::default(),
            grain: paper(grain::PAPER, 120.0, 0.8),
            ..chalk()
        },
    )
}

fn pastel_pencil() -> Brush {
    named(
        "Pastel Pencil",
        Brush {
            max_size: 26.0,
            shape: Shape { image: Some(shape::GRAINY_DISC), randomise: true, ..Shape::default() },
            grain: paper(grain::LINEN, 110.0, 0.7),
            ..chalk()
        },
    )
}

fn litho_crayon() -> Brush {
    named(
        "Litho Crayon",
        Brush {
            hardness: 0.9,
            max_size: 36.0,
            flow: 1.0,
            shape: Shape::default(),
            grain: paper(grain::COARSE, 160.0, 0.75),
            rendering: Rendering::Strongest,
            ..chalk()
        },
    )
}

// Watercolour

fn flat_wash() -> Brush {
    with(named("Flat Wash", Brush { max_size: 200.0, shape: along(shape::FLAT), ..wet_wash() }), &[("wet_edges", 0.5)])
}

fn dry_wash() -> Brush {
    with(
        named("Dry Wash", Brush { flow: 0.7, grain: paper(grain::COARSE, 300.0, 0.85), ..wet_wash() }),
        &[("wet_dilution", 0.3)],
    )
}

/// Pale blooms where salt drank the paint: a wash full of holes.
fn salt() -> Brush {
    with(
        named(
            "Salt",
            Brush {
                spacing: 0.3,
                max_size: 240.0,
                shape: Shape {
                    image: Some(shape::SPONGE),
                    randomise: true,
                    throw: 0.2,
                    size_jitter: 0.3,
                    ..Shape::default()
                },
                ..wet_wash()
            },
        ),
        &[("wet_edges", 0.6)],
    )
}

fn splash() -> Brush {
    with(
        named(
            "Splash",
            Brush {
                spacing: 0.8,
                max_size: 260.0,
                flow: 0.9,
                shape: Shape {
                    image: Some(shape::SPLATTER),
                    randomise: true,
                    throw: 0.8,
                    size_jitter: 0.6,
                    ..Shape::default()
                },
                ..wet_wash()
            },
        ),
        &[("wet_edges", 0.7), ("preview_size", 0.25)],
    )
}

fn mop_brush() -> Brush {
    with(
        named(
            "Mop Brush",
            Brush {
                hardness: 0.2,
                max_size: 400.0,
                pressure_size: 0.7,
                shape: Shape { image: Some(shape::SOFT_ROUND), ..Shape::default() },
                ..wet_wash()
            },
        ),
        &[("wet_dilution", 0.45), ("wet_edges", 0.35)],
    )
}

// Calligraphy

fn broad_pen() -> Brush {
    Brush { max_size: 90.0, ..nib("Broad Pen", 0.12, 0.0) }
}

fn italic_nib() -> Brush {
    nib("Italic Nib", 0.22, std::f32::consts::FRAC_PI_3)
}

fn felt_brush() -> Brush {
    named(
        "Felt Brush",
        Brush {
            hardness: 0.95,
            max_size: 50.0,
            pressure_size: 0.9,
            grain: paper(grain::FIBRE, 120.0, 0.3),
            ..script_brush()
        },
    )
}

fn copperplate() -> Brush {
    named("Copperplate", Brush { streamline: 0.6, min_size: 0.2, max_size: 28.0, ..pointed_nib() })
}

// Spray Paint

fn drips() -> Brush {
    let mut b = stamps("Drips", shape::DRIP, 1.0, 200.0, 0.6);
    b.shape.randomise = false;
    b.shape.opacity_jitter = 0.2;
    previewed(b, 0.25)
}

fn hard_spray() -> Brush {
    named(
        "Hard Spray",
        Brush {
            hardness: 0.9,
            flow: 0.6,
            shape: Shape::default(),
            grain: paper(grain::NOISE, 64.0, 0.6),
            ..spray_can()
        },
    )
}

fn splatter_can() -> Brush {
    named(
        "Splatter Can",
        Brush {
            spacing: 0.5,
            shape: Shape {
                image: Some(shape::SPLATTER),
                randomise: true,
                throw: 0.5,
                count: 2,
                size_jitter: 0.5,
                ..Shape::default()
            },
            ..spray_can()
        },
    )
}

fn skinny_cap() -> Brush {
    named("Skinny Cap", Brush { max_size: 60.0, flow: 0.95, ..spray_can() })
}

// Nature

/// A Nature stamp brush: `image` scattered, its colour shifting a little from stamp to stamp.
fn natural(name: &str, image: u32, spacing: f32, max_size: f32, throw: f32) -> Brush {
    previewed(with(stamps(name, image, spacing, max_size, throw), &NATURAL), 0.3)
}

fn pine() -> Brush {
    natural("Pine", shape::NEEDLES, 0.5, 200.0, 0.5)
}

fn twigs() -> Brush {
    natural("Twigs", shape::TWIG, 0.8, 220.0, 0.6)
}

fn moss() -> Brush {
    natural("Moss", shape::MOSS, 0.25, 160.0, 0.4)
}

fn flowers() -> Brush {
    natural("Flowers", shape::FLOWER, 0.9, 120.0, 0.9)
}

fn fur() -> Brush {
    with(
        named(
            "Fur",
            Brush {
                spacing: 0.12,
                streamline: 0.2,
                max_size: 160.0,
                flow: 0.9,
                pressure_size: 0.4,
                pressure_opacity: 0.3,
                shape: Shape { size_jitter: 0.3, ..along(shape::FUR) },
                pen_taper: Taper::NONE,
                touch_taper: Taper::NONE,
                ..Brush::round_pen()
            },
        ),
        &[("stamp_darkness", 0.2)],
    )
}

fn pebbles() -> Brush {
    natural("Pebbles", shape::STONE, 0.9, 120.0, 0.8)
}

fn feathers() -> Brush {
    natural("Feathers", shape::FEATHER, 1.2, 240.0, 0.7)
}

fn reeds() -> Brush {
    let mut b = natural("Reeds", shape::STREAK, 0.4, 260.0, 0.8);
    b.shape.randomise = false;
    b.shape.count = 3;
    b
}

// Effects

fn stars() -> Brush {
    previewed(stamps("Stars", shape::STAR, 1.2, 120.0, 1.2), 0.25)
}

fn bubbles() -> Brush {
    let mut b = stamps("Bubbles", shape::BUBBLE, 1.0, 200.0, 1.4);
    b.shape.size_jitter = 0.7;
    b.shape.opacity_jitter = 0.3;
    previewed(b, 0.2)
}

/// Sparks drifting off: small soft dots added to what is under them.
fn embers() -> Brush {
    let mut b = stamps("Embers", shape::SOFT_ROUND, 1.0, 40.0, 2.0);
    b.hardness = 0.3;
    b.shape.count = 2;
    b.shape.size_jitter = 0.8;
    b.shape.opacity_jitter = 0.6;
    previewed(with(b, &[("blend_mode", ADD)]), 0.4)
}

fn rain() -> Brush {
    let mut b = stamps("Rain", shape::STREAK, 1.0, 160.0, 1.5);
    b.shape.randomise = false;
    b.shape.rotation = 0.3;
    b.shape.count = 3;
    b.shape.opacity_jitter = 0.5;
    previewed(b, 0.3)
}

fn snowflakes() -> Brush {
    previewed(stamps("Snowflakes", shape::SNOWFLAKE, 1.2, 100.0, 1.4), 0.25)
}

// Pixel

fn pixel_checker() -> Brush {
    named("Pixel Checker", Brush { max_size: 200.0, grain: paper(grain::CHECKER, 32.0, 1.0), ..pixel() })
}

fn pixel_spray() -> Brush {
    named(
        "Pixel Spray",
        Brush {
            spacing: 0.5,
            max_size: 12.0,
            shape: Shape { image: Some(shape::SQUARE), throw: 3.0, count: 3, ..Shape::default() },
            ..pixel()
        },
    )
}

fn pixel_round() -> Brush {
    named("Pixel Round", Brush { shape: Shape::default(), ..pixel() })
}

fn pixel_line() -> Brush {
    named("Pixel Line", Brush { max_size: 4.0, streamline: 0.5, ..pixel() })
}

// ---- Elements ----

fn fire() -> Brush {
    let mut b = stamps("Fire", shape::FLAME, 0.35, 220.0, 0.35);
    b.shape.randomise = false;
    b.shape.count = 2;
    b.shape.opacity_jitter = 0.4;
    previewed(with(b, &[("stamp_hue", 0.05), ("stamp_lightness", 0.3)]), 0.3)
}

fn smoke_puff() -> Brush {
    named("Smoke Puff", Brush { flow: 0.2, hardness: 0.0, ..clouds() })
}

fn water_drops() -> Brush {
    let mut b = stamps("Water Drops", shape::DROPLET, 1.3, 90.0, 1.3);
    b.shape.randomise = false;
    previewed(b, 0.25)
}

fn ripples() -> Brush {
    texture_brush("Ripples", grain::CAUSTICS, 400.0)
}

/// Forked light: bolts end to end along the stroke, added to what is under them.
fn lightning() -> Brush {
    with(
        named(
            "Lightning",
            Brush {
                spacing: 1.0,
                streamline: 0.3,
                max_size: 300.0,
                pressure_size: 0.0,
                pressure_opacity: 0.0,
                shape: along(shape::BOLT),
                pen_taper: Taper::NONE,
                touch_taper: Taper::NONE,
                ..Brush::round_pen()
            },
        ),
        &[("blend_mode", ADD), ("preview_size", 0.3)],
    )
}

fn ice() -> Brush {
    previewed(stamps("Ice", shape::CRACK, 0.6, 200.0, 0.3), 0.3)
}

fn sea_foam() -> Brush {
    let mut b = stamps("Sea Foam", shape::SPONGE, 0.4, 200.0, 0.4);
    b.flow = 0.7;
    b.shape.count = 2;
    b
}

fn fog() -> Brush {
    named("Fog", Brush { max_size: 1200.0, flow: 0.08, spacing: 0.15, ..clouds() })
}

// ---- Industrial ----

fn rusted() -> Brush {
    let mut b = texture_brush("Rusted", grain::RUST, 300.0);
    b.shape = Shape { image: Some(shape::CHUNK), randomise: true, ..Shape::default() };
    b
}

fn cement() -> Brush {
    texture_brush("Cement", grain::CONCRETE, 180.0)
}

fn grunge() -> Brush {
    texture_brush("Grunge", grain::GRUNGE, 400.0)
}

fn scratched_metal() -> Brush {
    texture_brush("Scratched Metal", grain::SCRATCHES, 300.0)
}

fn steel_wool() -> Brush {
    let mut b = stamps("Steel Wool", shape::HAIR, 0.3, 160.0, 0.3);
    b.flow = 0.6;
    b.shape.count = 2;
    b.shape.size_jitter = 0.5;
    previewed(b, 0.3)
}

/// Evenly spaced round heads, as on a seam.
fn rivets() -> Brush {
    named(
        "Rivets",
        Brush {
            spacing: 2.2,
            streamline: 0.4,
            hardness: 0.9,
            max_size: 60.0,
            pressure_size: 0.0,
            shape: Shape { image: Some(shape::RING), ..Shape::default() },
            pen_taper: Taper::NONE,
            touch_taper: Taper::NONE,
            ..Brush::round_pen()
        },
    )
}

fn grime() -> Brush {
    named("Grime", Brush { grain: paper(grain::GRUNGE, 300.0, 0.9), flow: 0.6, ..spray_can() })
}

fn oil_stain() -> Brush {
    with(named("Oil Stain", Brush { flow: 0.5, ..blot() }), &[("blend_mode", MULTIPLY)])
}

// ---- Organic ----

fn hair() -> Brush {
    previewed(
        named(
            "Hair",
            Brush {
                spacing: 0.04,
                streamline: 0.5,
                max_size: 120.0,
                flow: 0.8,
                pressure_size: 0.5,
                pressure_opacity: 0.4,
                shape: along(shape::HAIR),
                pen_taper: Taper { start: 0.2, end: 0.3, size: 0.6, opacity: 0.4 },
                rendering: Rendering::Strongest,
                ..Brush::round_pen()
            },
        ),
        0.3,
    )
}

fn skin_pores() -> Brush {
    Brush { flow: 0.5, ..texture_brush("Skin Pores", grain::LEATHER, 90.0) }
}

fn freckles() -> Brush {
    let mut b = stamps("Freckles", shape::SOFT_ROUND, 0.8, 16.0, 1.5);
    b.hardness = 0.5;
    b.shape.count = 2;
    b.shape.size_jitter = 0.7;
    b.shape.opacity_jitter = 0.6;
    previewed(b, 0.4)
}

fn reptile() -> Brush {
    texture_brush("Reptile", grain::SCALES, 120.0)
}

fn coral() -> Brush {
    previewed(with(stamps("Coral", shape::SPONGE, 0.5, 200.0, 0.5), &NATURAL), 0.3)
}

fn lichen() -> Brush {
    natural("Lichen", shape::MOSS, 0.6, 120.0, 0.9)
}

fn seeds() -> Brush {
    natural("Seeds", shape::DIAMOND, 1.0, 30.0, 1.5)
}

fn veins() -> Brush {
    let mut b = stamps("Veins", shape::CRACK, 0.7, 150.0, 0.2);
    b.flow = 0.8;
    previewed(b, 0.3)
}

// ---- Abstract ----

fn hearts() -> Brush {
    previewed(stamps("Hearts", shape::HEART, 1.2, 100.0, 1.0), 0.25)
}

fn confetti() -> Brush {
    previewed(with(stamps("Confetti", shape::CONFETTI, 0.8, 200.0, 0.8), &[("stamp_hue", 1.0)]), 0.25)
}

fn triangles() -> Brush {
    previewed(stamps("Triangles", shape::TRIANGLE, 1.2, 90.0, 1.0), 0.25)
}

fn diamonds() -> Brush {
    previewed(stamps("Diamonds", shape::DIAMOND, 1.2, 90.0, 1.0), 0.25)
}

fn speed_lines() -> Brush {
    with(
        named(
            "Speed Lines",
            Brush {
                spacing: 0.6,
                streamline: 0.5,
                max_size: 300.0,
                pressure_size: 0.0,
                pressure_opacity: 0.0,
                shape: Shape { count: 3, opacity_jitter: 0.5, size_jitter: 0.5, ..along(shape::STREAK) },
                pen_taper: Taper::NONE,
                touch_taper: Taper::NONE,
                ..Brush::round_pen()
            },
        ),
        &[("jitter_lateral", 0.6)],
    )
}

fn scribbles() -> Brush {
    previewed(stamps("Scribbles", shape::SCRIBBLE, 0.7, 160.0, 0.4), 0.3)
}

/// A dotted line: round dots a little apart.
fn dot_trail() -> Brush {
    named(
        "Dot Trail",
        Brush {
            spacing: 1.6,
            streamline: 0.5,
            hardness: 1.0,
            max_size: 50.0,
            pressure_size: 0.3,
            pen_taper: Taper::NONE,
            touch_taper: Taper::NONE,
            ..Brush::round_pen()
        },
    )
}

/// A dashed line: short bars along the stroke.
fn dashes() -> Brush {
    named(
        "Dashes",
        Brush {
            shape: Shape { image: Some(shape::SQUARE), roundness: 0.3, follow: 1.0, ..Shape::default() },
            spacing: 2.0,
            ..dot_trail()
        },
    )
}

// ---- Touchups ----

fn soft_blender() -> Brush {
    named(
        "Soft Blender",
        Brush {
            spacing: 0.05,
            streamline: 0.2,
            hardness: 0.0,
            max_size: 300.0,
            flow: 0.2,
            pressure_size: 0.3,
            pressure_opacity: 0.7,
            shape: Shape { image: Some(shape::SOFT_ROUND), ..Shape::default() },
            pen_taper: Taper::NONE,
            touch_taper: Taper::NONE,
            ..Brush::round_pen()
        },
    )
}

fn grainy_blender() -> Brush {
    named("Grainy Blender", Brush { grain: paper(grain::PAPER, 160.0, 0.6), flow: 0.3, ..soft_blender() })
}

fn retouch() -> Brush {
    named("Retouch", Brush { max_size: 60.0, hardness: 0.4, flow: 0.4, ..soft_blender() })
}

fn highlight() -> Brush {
    with(named("Highlight", Brush { flow: 0.3, ..soft_blender() }), &[("blend_mode", SCREEN)])
}

fn shadow() -> Brush {
    with(named("Shadow", Brush { flow: 0.25, ..soft_blender() }), &[("blend_mode", MULTIPLY)])
}

fn noise() -> Brush {
    named("Noise", Brush { grain: paper(grain::NOISE, 96.0, 1.0), flow: 0.3, ..soft_blender() })
}

fn fine_detail() -> Brush {
    named(
        "Fine Detail",
        Brush {
            streamline: 0.3,
            hardness: 0.85,
            max_size: 12.0,
            flow: 0.9,
            pressure_size: 0.6,
            pressure_opacity: 0.4,
            ..Brush::round_pen()
        },
    )
}

// ---- Retro ----

fn ben_day() -> Brush {
    texture_brush("Ben Day", grain::HALFTONE, 200.0)
}

fn screen_tone() -> Brush {
    texture_brush("Screen Tone", grain::HALFTONE, 48.0)
}

fn comic_ink() -> Brush {
    named("Comic Ink", Brush { pressure_curve: Curve::linear(), max_size: 40.0, ..studio_ink() })
}

fn retro_stipple() -> Brush {
    Brush { hardness: 0.95, ..texture_brush("Retro Stipple", grain::STIPPLE, 120.0) }
}

fn pop_dots() -> Brush {
    named("Pop Dots", Brush { spacing: 1.2, max_size: 80.0, pressure_size: 0.6, ..dot_trail() })
}

fn retro_lines() -> Brush {
    texture_brush("Retro Lines", grain::STRIPES, 64.0)
}

/// Colour printed in dots, darkening what is under it.
fn offset_print() -> Brush {
    with(
        named(
            "Offset Print",
            Brush { hardness: 0.9, flow: 0.9, grain: paper(grain::HALFTONE, 80.0, 0.6), ..round_brush() },
        ),
        &[("blend_mode", MULTIPLY)],
    )
}

// ---- Vintage ----

fn worn_ink() -> Brush {
    named("Worn Ink", Brush { grain: paper(grain::GRUNGE, 260.0, 0.7), ..dry_ink() })
}

fn letterpress() -> Brush {
    named("Letterpress", Brush { hardness: 1.0, grain: paper(grain::SAND, 200.0, 0.5), ..monoline() })
}

fn old_paper() -> Brush {
    Brush { flow: 0.4, ..texture_brush("Old Paper", grain::CRUMPLED, 500.0) }
}

fn sepia_wash() -> Brush {
    with(
        named("Sepia Wash", Brush { grain: paper(grain::WASH, 300.0, 0.6), flow: 0.6, ..wet_wash() }),
        &[("blend_mode", MULTIPLY)],
    )
}

fn scratchy_pen() -> Brush {
    named("Scratchy Pen", Brush { grain: paper(grain::SCRATCHES, 160.0, 0.6), ..dip_pen() })
}

fn rubber_stamp() -> Brush {
    Brush { hardness: 0.95, shape: Shape::default(), ..texture_brush("Rubber Stamp", grain::GRUNGE, 200.0) }
}

fn faded_print() -> Brush {
    Brush { flow: 0.5, ..texture_brush("Faded Print", grain::RUST, 260.0) }
}

// ---- Luminance ----

/// A bright core in a soft glow, added to what is under it.
fn neon() -> Brush {
    with(
        named(
            "Neon",
            Brush {
                spacing: 0.04,
                streamline: 0.5,
                hardness: 0.0,
                max_size: 80.0,
                flow: 0.5,
                pressure_size: 0.3,
                pressure_opacity: 0.0,
                shape: Shape { image: Some(shape::SOFT_ROUND), ..Shape::default() },
                pen_taper: Taper::NONE,
                touch_taper: Taper::NONE,
                ..Brush::round_pen()
            },
        ),
        &[("blend_mode", ADD)],
    )
}

fn glimmer() -> Brush {
    with(named("Glimmer", Brush { max_size: 80.0, ..sparkle() }), &[("blend_mode", ADD)])
}

fn lens_dust() -> Brush {
    let mut b = stamps("Lens Dust", shape::RING, 1.0, 300.0, 1.8);
    b.flow = 0.35;
    b.shape.size_jitter = 0.8;
    b.shape.opacity_jitter = 0.7;
    previewed(with(b, &[("blend_mode", SCREEN)]), 0.2)
}

fn flare() -> Brush {
    named("Flare", Brush { shape: Shape { image: Some(shape::SPARKLE), ..Shape::default() }, ..glow() })
}

fn halo() -> Brush {
    with(named("Halo", Brush { flow: 0.4, ..bokeh() }), &[("blend_mode", SCREEN)])
}

fn light_leak() -> Brush {
    with(named("Light Leak", Brush { max_size: 1500.0, flow: 0.12, ..glow() }), &[("blend_mode", SCREEN)])
}

fn aurora() -> Brush {
    with(named("Aurora", Brush { flow: 0.3, ..smoke() }), &[("blend_mode", ADD), ("stamp_hue", 0.1)])
}

fn fairy_dust() -> Brush {
    with(
        named("Fairy Dust", Brush { flow: 0.8, grain: paper(grain::SPECKLE, 160.0, 1.0), ..mist() }),
        &[("blend_mode", DODGE)],
    )
}

fn soft_glow() -> Brush {
    with(named("Soft Glow", Brush { flow: 0.25, ..glow() }), &[("blend_mode", SOFT_LIGHT)])
}

fn overlay_light() -> Brush {
    with(named("Overlay Light", Brush { flow: 0.3, ..glow() }), &[("blend_mode", OVERLAY)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::textures::{grains, shapes};

    #[test]
    fn library_is_complete_and_consistent() {
        let sets = library();
        assert!(sets.len() >= 6);
        let mut shelf = SHELF.to_vec();
        shelf.sort_unstable();
        assert_eq!(shelf, (0..sets.len()).collect::<Vec<_>>(), "the shelf shows every set once");
        let (shape_count, grain_count) = (shapes().len() as u32, grains().len() as u32);
        let mut names = std::collections::HashSet::new();
        for set in &sets {
            assert!(!set.brushes.is_empty(), "{}", set.name);
            for b in &set.brushes {
                assert!(names.insert(b.name.clone()), "duplicate brush name {}", b.name);
                assert!(b.shape.image.is_none_or(|i| i < shape_count), "{}", b.name);
                assert!(b.grain.image.is_none_or(|i| i < grain_count), "{}", b.name);
                assert!(b.min_size < b.max_size && b.spacing > 0.0 && b.flow > 0.0, "{}", b.name);
                assert!(b.grain.image.is_none() || b.grain.scale > 0.0, "{}", b.name);
            }
        }
    }
}
