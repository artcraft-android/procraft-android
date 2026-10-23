//! Brush shape and grain images, generated here (ProCraft's own, made procedurally at start-up):
//! 8-bit greyscale, white = paint. Grains tile seamlessly.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// Edge of every shape and grain image, pixels.
pub const TEXTURE_SIZE: usize = 256;

/// A square greyscale image, `TEXTURE_SIZE` on a side, rows top to bottom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrayImage {
    pub name: &'static str,
    pub pixels: Vec<u8>,
}

/// Shapes, by index (brushes refer to them by index).
pub mod shape {
    pub const SOFT_ROUND: u32 = 0;
    pub const GRAINY_DISC: u32 = 1;
    pub const CHUNK: u32 = 2;
    pub const BRISTLES: u32 = 3;
    pub const SPLATTER: u32 = 4;
    pub const FLAT: u32 = 5;
    pub const PASTEL: u32 = 6;
    pub const BLOT: u32 = 7;
    pub const LEAF: u32 = 8;
    pub const GRASS: u32 = 9;
    pub const SPARKLE: u32 = 10;
    pub const CLOUD: u32 = 11;
    pub const SQUARE: u32 = 12;
    pub const DRY: u32 = 13;
    pub const RING: u32 = 14;
    pub const SPRAY: u32 = 15;
    pub const HATCH: u32 = 16;
    pub const WISP: u32 = 17;
    pub const STAR: u32 = 18;
    pub const FUR: u32 = 19;
    pub const DROPLET: u32 = 20;
    pub const FLAME: u32 = 21;
    pub const BUBBLE: u32 = 22;
    pub const STONE: u32 = 23;
    pub const STREAK: u32 = 24;
    pub const FLOWER: u32 = 25;
    pub const TWIG: u32 = 26;
    pub const CRACK: u32 = 27;
    pub const SPONGE: u32 = 28;
    pub const TRIANGLE: u32 = 29;
    pub const DIAMOND: u32 = 30;
    pub const NEEDLES: u32 = 31;
    pub const FEATHER: u32 = 32;
    pub const CONFETTI: u32 = 33;
    pub const HAIR: u32 = 34;
    pub const SMUDGE: u32 = 35;
    pub const MOSS: u32 = 36;
    pub const HEART: u32 = 37;
    pub const SNOWFLAKE: u32 = 38;
    pub const SCRIBBLE: u32 = 39;
    pub const DRIP: u32 = 40;
    pub const BOLT: u32 = 41;
    pub const BLOCK: u32 = 42;
    pub const STOCK_GRAINY_DISC: u32 = 43;
    pub const STOCK_CHUNK: u32 = 44;
    pub const STOCK_PASTEL: u32 = 45;
    pub const STOCK_DAB: u32 = 46;
}

/// Grains, by index.
pub mod grain {
    pub const PAPER: u32 = 0;
    pub const COARSE: u32 = 1;
    pub const WEAVE: u32 = 2;
    pub const FIBRE: u32 = 3;
    pub const LINEN: u32 = 4;
    pub const WASH: u32 = 5;
    pub const CONCRETE: u32 = 6;
    pub const HALFTONE: u32 = 7;
    pub const HATCHING: u32 = 8;
    pub const NOISE: u32 = 9;
    pub const CRUMPLED: u32 = 10;
    pub const STIPPLE: u32 = 11;
    pub const WOOD: u32 = 12;
    pub const BRICK: u32 = 13;
    pub const SCALES: u32 = 14;
    pub const CELLS: u32 = 15;
    pub const MARBLE: u32 = 16;
    pub const STRIPES: u32 = 17;
    pub const GRUNGE: u32 = 18;
    pub const SAND: u32 = 19;
    pub const HEXAGONS: u32 = 20;
    pub const CAUSTICS: u32 = 21;
    pub const LEATHER: u32 = 22;
    pub const BARK: u32 = 23;
    pub const SCRATCHES: u32 = 24;
    pub const CHECKER: u32 = 25;
    pub const SPECKLE: u32 = 26;
    pub const TWILL: u32 = 27;
    pub const RUST: u32 = 28;
    pub const RECYCLED: u32 = 29;
    pub const SHEET: u32 = 30;
    pub const WORN: u32 = 31;
    pub const CHARCOAL: u32 = 32;
    pub const FINE_NOISE: u32 = 33;
    pub const SPECKLED: u32 = 34;
    pub const MOTTLED: u32 = 35;
}

/// The Coarse grain's tooth: below this the fibres darken it, by this much a unit.
const COARSE_LIGHT: f32 = 0.6;
const COARSE_DEPTH: f32 = 1.25;

/// The shapes, made once.
pub fn shapes() -> Vec<GrayImage> {
    static MADE: std::sync::OnceLock<Vec<GrayImage>> = std::sync::OnceLock::new();
    MADE.get_or_init(make_shapes).clone()
}

/// The grains, made once.
pub fn grains() -> Vec<GrayImage> {
    static MADE: std::sync::OnceLock<Vec<GrayImage>> = std::sync::OnceLock::new();
    MADE.get_or_init(make_grains).clone()
}

fn make_shapes() -> Vec<GrayImage> {
    let mut shapes = vec![
        // full well out towards its edge: stamped along a stroke it builds up to a soft line most of
        // its width across, with a short fade at each side
        GrayImage { name: "Soft round", pixels: field(|x, y| smooth(1.0 - radius(x, y)).powf(0.62)) },
        GrayImage {
            name: "Grainy disc",
            pixels: field(|x, y| {
                let edge = 1.0 - radius(x, y);
                let n = fbm(x * 12.0, y * 12.0, 12, 3, 11);
                (edge * 6.0).clamp(0.0, 1.0) * (0.55 + 0.45 * n).clamp(0.0, 1.0) * if n > 0.25 { 1.0 } else { n * 4.0 }
            }),
        },
        GrayImage {
            name: "Chunk",
            pixels: field(|x, y| {
                let n = fbm(x * 4.0, y * 4.0, 4, 4, 23);
                let r = radius(x, y) + (n - 0.5) * 0.55;
                let edge = smooth(((1.0 - r) * 5.0).clamp(0.0, 1.0));
                edge * (0.6 + 0.4 * fbm(x * 16.0, y * 16.0, 16, 2, 7))
            }),
        },
        GrayImage {
            name: "Bristles",
            pixels: field(|x, y| {
                let across = (x - 0.5).abs() * 2.0;
                let along = (y - 0.5).abs() * 2.0;
                let hairs = 0.5 + 0.5 * (x * 60.0 + fbm(x * 8.0, y, 8, 2, 5) * 6.0).sin();
                let body =
                    smooth(((1.0 - across) * 4.0).clamp(0.0, 1.0)) * smooth(((1.0 - along) * 2.5).clamp(0.0, 1.0));
                body * (0.35 + 0.65 * hairs)
            }),
        },
        GrayImage {
            name: "Splatter",
            pixels: {
                let dots = scatter_points(41, 26);
                field(|x, y| {
                    dots.iter().fold(0.0f32, |acc, &(cx, cy, r)| {
                        let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() / r;
                        acc.max(smooth(((1.0 - d) * 3.0).clamp(0.0, 1.0)))
                    })
                })
            },
        },
        GrayImage {
            name: "Flat",
            pixels: field(|x, y| {
                let across = (x - 0.5).abs() * 2.0;
                let along = (y - 0.5).abs() * 2.0;
                smooth(((1.0 - across) * 3.0).clamp(0.0, 1.0)) * smooth(((1.0 - along) * 6.0).clamp(0.0, 1.0))
            }),
        },
        GrayImage {
            name: "Pastel",
            pixels: field(|x, y| {
                // a worn block: squarish, ragged at its edge, patchy inside
                let n = fbm(x * 6.0, y * 6.0, 6, 4, 31);
                let block = 1.0 - ((x - 0.5).abs() * 2.0).max((y - 0.5).abs() * 2.4);
                let edge = smooth(((block - 0.12 + (n - 0.5) * 0.45) * 5.0).clamp(0.0, 1.0));
                edge * contrast(fbm(x * 24.0, y * 24.0, 24, 3, 37), 1.8).max(0.25)
            }),
        },
        GrayImage {
            name: "Blot",
            pixels: {
                let drops = scatter_points(53, 9);
                field(move |x, y| {
                    // spread over two thirds of the square, as a stock ink blot's mass
                    let (x, y) = (0.5 + (x - 0.5) / 1.22, 0.5 + (y - 0.5) / 1.22);
                    let n = fbm(x * 3.0, y * 3.0, 3, 3, 41);
                    let body = smooth(((0.62 - radius(x, y) - (n - 0.5) * 0.5) * 9.0).clamp(0.0, 1.0));
                    let spots = drops.iter().fold(0.0f32, |acc, &(cx, cy, r)| {
                        // pushed out towards the edge
                        let (dx, dy) = ((cx - 0.5) * 2.2 + 0.5, (cy - 0.5) * 2.2 + 0.5);
                        let d = ((x - dx).powi(2) + (y - dy).powi(2)).sqrt() / (r * 0.6);
                        acc.max(smooth(((1.0 - d) * 4.0).clamp(0.0, 1.0)))
                    });
                    body.max(spots) * (1.0 - 0.05 * radius(x, y))
                })
            },
        },
        GrayImage {
            name: "Leaf",
            pixels: field(|x, y| {
                // pointing up, widest below the middle, a pale midrib and a short stem
                let (u, v) = ((x - 0.5) * 2.0, (y - 0.5) * 2.0);
                let t = ((v + 0.85) / 1.5).clamp(0.0, 1.0);
                let width = 0.52 * (t * std::f32::consts::PI).sin().powf(0.8) * (1.0 - 0.25 * t);
                let blade = smooth(((width - u.abs()) * 30.0).clamp(0.0, 1.0)) * f32::from(u8::from(v < 0.65));
                let rib =
                    1.0 - 0.5 * smooth(((0.025 - u.abs()) * 60.0).clamp(0.0, 1.0)) * f32::from(u8::from(v > -0.6));
                let stem = smooth(((0.03 - u.abs()) * 60.0).clamp(0.0, 1.0)) * f32::from(u8::from(v > 0.6 && v < 0.9));
                (blade * rib).max(stem)
            }),
        },
        GrayImage {
            name: "Grass",
            pixels: field(|x, y| {
                // blades from the bottom, each leaning and narrowing to its tip
                (0..11).fold(0.0f32, |acc, k| {
                    let base = 0.12 + 0.76 * hash(k, 1, 61);
                    let lean = (hash(k, 2, 61) - 0.5) * 0.5;
                    let top = 0.08 + 0.35 * hash(k, 3, 61);
                    let along = ((0.92 - y) / (0.92 - top)).clamp(0.0, 1.0);
                    if y < top || y > 0.92 {
                        return acc;
                    }
                    let cx = base + lean * along * along;
                    let half = 0.03 * (1.0 - along).powf(0.7);
                    acc.max(smooth(((half - (x - cx).abs()) * 160.0).clamp(0.0, 1.0)))
                })
            }),
        },
        GrayImage {
            name: "Sparkle",
            pixels: field(|x, y| {
                // four long points and four short ones, each narrowing to its tip, over a small glow
                let (u, v) = ((x - 0.5).abs() * 2.0, (y - 0.5).abs() * 2.0);
                let point = |along: f32, off: f32| {
                    let width = 0.09 * (1.0 - along).max(0.0).powf(1.4) + 0.004;
                    smooth(1.0 - off / width) * f32::from(u8::from(along < 1.0))
                };
                let long = point(u, v).max(point(v, u));
                let (a, b) =
                    ((u + v) * std::f32::consts::FRAC_1_SQRT_2, (u - v).abs() * std::f32::consts::FRAC_1_SQRT_2);
                let short = point(a * 1.8, b * 1.4) * 0.8;
                let glow = (-(u * u + v * v) * 40.0).exp();
                long.max(short).max(glow).min(1.0) * smooth(((1.0 - radius(x, y)) * 8.0).clamp(0.0, 1.0))
            }),
        },
        GrayImage {
            name: "Cloud",
            pixels: {
                let puffs = scatter_points(71, 9);
                field(move |x, y| {
                    // puffs drawn in towards the middle, overlapping into one cloud
                    let lumps = puffs.iter().fold(0.0f32, |acc, &(cx, cy, r)| {
                        let (px, py) = (0.5 + (cx - 0.5) * 0.75, 0.5 + (cy - 0.5) * 0.6);
                        let d = ((x - px).powi(2) + (y - py).powi(2)).sqrt() / (r * 1.5 + 0.2);
                        acc.max(smooth((1.0 - d).clamp(0.0, 1.0) * 1.6))
                    });
                    let n = fbm(x * 8.0, y * 8.0, 8, 4, 73);
                    (lumps * (0.6 + 0.55 * n)).min(1.0) * smooth(((1.0 - radius(x, y)) * 4.0).clamp(0.0, 1.0))
                })
            },
        },
        GrayImage {
            name: "Square",
            pixels: field(|x, y| f32::from(u8::from((x - 0.5).abs() < 0.4 && (y - 0.5).abs() < 0.4))),
        },
        GrayImage {
            name: "Dry",
            pixels: field(|x, y| {
                // a flat stroke's worth of hairs, broken where the paint ran out
                let across = (x - 0.5).abs() * 2.0;
                let along = (y - 0.5).abs() * 2.0;
                let hair = value_noise(x * 90.0, y * 2.0, 90, 83);
                let broken = smooth(((hair - 0.35) * 5.0).clamp(0.0, 1.0));
                let body =
                    smooth(((1.0 - across) * 6.0).clamp(0.0, 1.0)) * smooth(((1.0 - along) * 3.0).clamp(0.0, 1.0));
                body * broken
            }),
        },
        GrayImage {
            name: "Ring",
            pixels: field(|x, y| {
                let r = radius(x, y);
                let band = smooth(((0.06 - (r - 0.82).abs()) * 40.0).clamp(0.0, 1.0));
                band.max(0.25 * smooth(((0.8 - r) * 10.0).clamp(0.0, 1.0)))
            }),
        },
        GrayImage {
            name: "Spray",
            pixels: {
                // fine droplets, thick in the middle and thinning out
                let drops: Vec<(f32, f32, f32)> = (0..420)
                    .map(|i| {
                        let a = hash(i, 1, 97) * std::f32::consts::TAU;
                        let d = (hash(i, 2, 97) * hash(i, 4, 97)).sqrt() * 0.46;
                        (0.5 + a.cos() * d, 0.5 + a.sin() * d, 0.004 + 0.006 * hash(i, 3, 97))
                    })
                    .collect();
                field(move |x, y| {
                    drops.iter().fold(0.0f32, |acc, &(cx, cy, r)| {
                        let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() / r;
                        acc.max((1.5 - d).clamp(0.0, 1.0))
                    })
                })
            },
        },
        GrayImage {
            name: "Hatch",
            pixels: field(|x, y| {
                let lines = 0.5 + 0.5 * ((x + y) * std::f32::consts::TAU * 9.0).cos();
                let ink = smooth(((lines - 0.55) * 6.0).clamp(0.0, 1.0));
                ink * smooth(((1.0 - radius(x, y)) * 5.0).clamp(0.0, 1.0))
            }),
        },
        GrayImage {
            name: "Wisp",
            pixels: field(|x, y| {
                // bands bent by noise: smoke curling
                let warp = fbm(x * 3.0, y * 3.0, 3, 3, 109);
                let bands = 0.5 + 0.5 * ((y + warp * 0.6) * std::f32::consts::TAU * 3.0).sin();
                let soft = smooth(((1.0 - radius(x, y)) * 2.0).clamp(0.0, 1.0));
                (bands.powi(3) * soft * (0.6 + 0.6 * fbm(x * 6.0, y * 6.0, 6, 3, 113))).min(1.0)
            }),
        },
        GrayImage {
            name: "Star",
            pixels: field(|x, y| {
                // five points, one straight up, the edges between them curving in
                let (u, v) = centred(x, y);
                let s = ((v.atan2(u) + FRAC_PI_2) / TAU * 5.0).rem_euclid(1.0);
                let edge = 0.4 + 0.52 * ((s - 0.5).abs() * 2.0).powf(1.6);
                smooth((edge - (u * u + v * v).sqrt()) * 40.0)
            }),
        },
        GrayImage {
            name: "Fur",
            pixels: field(|x, y| {
                // fine hairs from a band low down, bending as they rise and narrowing to their tips
                (0..48).fold(0.0f32, |acc, k| {
                    let (foot, top) = (0.75 + 0.12 * hash(k, 2, 131), 0.1 + 0.3 * hash(k, 3, 131));
                    if y < top || y > foot {
                        return acc;
                    }
                    let along = (foot - y) / (foot - top);
                    let cx = 0.15 + 0.7 * hash(k, 1, 131) + (hash(k, 4, 131) - 0.5) * 0.3 * along * along;
                    let half = 0.008 * (1.0 - along).powf(0.6) + 0.001;
                    acc.max(smooth((half - (x - cx).abs()) * 400.0) * (0.5 + 0.5 * hash(k, 5, 131)))
                })
            }),
        },
        GrayImage {
            name: "Droplet",
            pixels: field(|x, y| {
                // round below and drawn up to a point; a strong rim, a paler middle, a bright spot
                let (u, v) = centred(x, y);
                let (cy, r) = (0.3, 0.58);
                let width = if v >= cy {
                    (r * r - (v - cy).powi(2)).max(0.0).sqrt()
                } else {
                    r * ((v + 0.92) / (cy + 0.92)).clamp(0.0, 1.0).powf(1.5)
                };
                let inside = width - u.abs();
                let rim = 0.55 + 0.45 * smooth((0.12 - inside) * 12.0);
                let spot = smooth((0.1 - ((u + 0.2).powi(2) + (v - 0.18).powi(2)).sqrt()) * 30.0);
                smooth(inside * 30.0) * rim * (1.0 - 0.8 * spot)
            }),
        },
        GrayImage {
            name: "Flame",
            pixels: field(|x, y| {
                // a tongue of fire: round and full at its foot, flickering up to a point
                let (u, v) = centred(x, y);
                let t = ((0.85 - v) / 1.8).clamp(0.0, 1.0);
                let width = if t < 0.2 {
                    0.46 * (1.0 - ((0.2 - t) / 0.2).powi(2)).max(0.0).sqrt()
                } else {
                    0.46 * ((1.0 - t) / 0.8).powf(1.5)
                };
                // the upper part sways and frays into tongues
                let sway = (fbm(x * 2.0, y * 4.0, 2, 3, 137) - 0.5) * 0.9 * t.powf(1.2);
                let fray = (fbm(x * 6.0, y * 3.0, 3, 3, 139) - 0.5) * 0.35 * t;
                let body = smooth((width + fray - (u - sway).abs()) * 18.0);
                let flicker = 0.75 + 0.35 * fbm(x * 8.0, y * 8.0, 8, 3, 141);
                (body * (0.6 + 0.4 * (1.0 - t)) * flicker).min(1.0)
            }),
        },
        GrayImage {
            name: "Bubble",
            pixels: field(|x, y| {
                // a thin skin, a faint inside and a curved highlight to the upper left
                let r = radius(x, y);
                let skin = smooth((0.05 - (r - 0.86).abs()) * 50.0);
                let inside = 0.12 * smooth((0.86 - r) * 10.0);
                let a = (y - 0.5).atan2(x - 0.5);
                let arc = smooth(1.0 - ((a + 2.36) / 0.6).abs()) * smooth((0.05 - (r - 0.66).abs()) * 40.0);
                skin.max(inside).max(arc * 0.9)
            }),
        },
        GrayImage {
            name: "Stone",
            pixels: {
                // an angular pebble: straight edges between seven corners, lit from the upper left
                let corners: Vec<(f32, f32)> = (0..7)
                    .map(|k| {
                        let a = (k as f32 + 0.4 * hash(k, 1, 139)) / 7.0 * TAU;
                        let r = 0.62 + 0.3 * hash(k, 2, 139);
                        (a.cos() * r, a.sin() * r)
                    })
                    .collect();
                field(move |x, y| {
                    let (u, v) = centred(x, y);
                    let (edge, inside) = polygon(&corners, u, v);
                    if !inside {
                        return 0.0;
                    }
                    let lit = 0.85 - 0.15 * (u + v);
                    smooth(edge * 30.0) * (lit * (0.75 + 0.3 * fbm(x * 10.0, y * 10.0, 10, 3, 141))).min(1.0)
                })
            },
        },
        GrayImage {
            name: "Streak",
            pixels: field(|x, y| {
                // a long thin line, thickest a little below the middle, fading out at both ends
                let (u, v) = centred(x, y);
                let half = 0.035 * (1.0 - ((v - 0.05) / 0.85).powi(2)).max(0.0);
                smooth((half - u.abs()) * 100.0)
            }),
        },
        GrayImage {
            name: "Flower",
            pixels: field(|x, y| {
                // five rounded petals round a paler middle
                let (u, v) = centred(x, y);
                let (r, a) = ((u * u + v * v).sqrt(), v.atan2(u) + FRAC_PI_2);
                let petal = (a * 2.5).cos().abs();
                let body = smooth((0.3 + 0.6 * petal.sqrt() - r) * 25.0);
                let middle = 1.0 - 0.45 * smooth((0.2 - r) * 20.0);
                body * (0.8 + 0.2 * petal) * middle
            }),
        },
        GrayImage {
            name: "Twig",
            pixels: {
                // a stick with three side shoots, each thinning to its end
                let parts = [
                    ((0.02, 0.92), (-0.04, -0.88), 0.07, 0.025),
                    ((0.0, 0.35), (-0.55, -0.25), 0.04, 0.012),
                    ((-0.02, -0.15), (0.5, -0.6), 0.035, 0.01),
                    ((-0.03, -0.5), (-0.3, -0.85), 0.025, 0.008),
                ];
                field(move |x, y| {
                    let (u, v) = centred(x, y);
                    parts.iter().fold(0.0f32, |acc, &(a, b, w0, w1)| {
                        let (d, t) = segment(u, v, a, b);
                        acc.max(smooth((w0 + (w1 - w0) * t - d) * 60.0))
                    }) * (0.8 + 0.25 * fbm(x * 4.0, y * 32.0, 4, 2, 143)).min(1.0)
                })
            },
        },
        GrayImage {
            name: "Crack",
            pixels: {
                let lines = cracks(149);
                field(move |x, y| {
                    let (u, v) = centred(x, y);
                    lines.iter().fold(0.0f32, |acc, &(a, b, w)| acc.max(smooth((w - segment(u, v, a, b).0) * 120.0)))
                })
            },
        },
        GrayImage {
            name: "Sponge",
            pixels: field(|x, y| {
                // a round dab full of holes of every size
                let n = fbm(x * 4.0, y * 4.0, 4, 2, 151);
                let body = smooth((0.9 - radius(x, y) - (n - 0.5) * 0.3) * 8.0);
                let (near, _) = cells(x, y, 10, 157);
                let (fine, _) = cells(x, y, 22, 163);
                body * smooth((near - 0.28) * 6.0) * smooth((fine - 0.22) * 6.0)
            }),
        },
        GrayImage {
            name: "Triangle",
            pixels: field(|x, y| {
                // pointing up, its middle at the centre
                let (u, v) = centred(x, y);
                let d = (v - 0.45).max(0.866 * u.abs() - 0.5 * v - 0.45);
                smooth(-d * 60.0)
            }),
        },
        GrayImage {
            name: "Diamond",
            pixels: field(|x, y| {
                let (u, v) = centred(x, y);
                smooth((1.0 - u.abs() / 0.6 - v.abs() / 0.95) * 20.0)
            }),
        },
        GrayImage {
            name: "Needles",
            pixels: {
                // a sprig: a stem with pairs of needles slanting up, shorter towards its tip
                let mut parts = vec![((0.0, 0.92), (0.0, -0.9), 0.025)];
                for k in 0..13 {
                    let at = 0.8 - k as f32 * 0.13;
                    let len = 0.25 + 0.25 * (at + 0.9) / 1.7;
                    for side in [-1.0f32, 1.0] {
                        parts.push(((0.0, at), (side * len * 0.77, at - len * 0.64), 0.012));
                    }
                }
                field(move |x, y| {
                    let (u, v) = centred(x, y);
                    parts.iter().fold(0.0f32, |acc, &(a, b, w)| {
                        let (d, t) = segment(u, v, a, b);
                        acc.max(smooth((w * (1.0 - 0.6 * t) - d) * 150.0))
                    })
                })
            },
        },
        GrayImage {
            name: "Feather",
            pixels: field(|x, y| {
                // a vane round a quill, its barbs slanting out and up, split here and there
                let (u, v) = centred(x, y);
                let t = (v + 0.95) / 1.85;
                let vane = 0.42 * (PI * (t / 0.82).clamp(0.0, 1.0)).sin().powf(0.7);
                let lean = u + 0.08 * (t * PI).sin();
                let barbs = 0.5 + 0.5 * ((v + lean.abs() * 0.8) * TAU * 14.0).cos();
                let split = smooth((value_noise((v + lean.abs() * 0.8) * 9.0, u.signum(), 9, 167) - 0.2) * 8.0);
                let body = smooth((vane - lean.abs()) * 30.0) * (0.55 + 0.45 * barbs) * split;
                let quill = smooth((0.018 * (1.0 - 0.3 * t) - lean.abs()) * 120.0) * f32::from(u8::from(t < 1.0));
                body.max(quill)
            }),
        },
        GrayImage {
            name: "Confetti",
            pixels: {
                // little paper strips turned every way
                let strips: Vec<(f32, f32, f32, f32, f32, f32)> = (0..22)
                    .map(|i| {
                        let a = hash(i, 1, 173) * TAU;
                        let d = hash(i, 2, 173).sqrt() * 0.36;
                        let turn = hash(i, 3, 173) * PI;
                        let (w, h) = (0.012 + 0.012 * hash(i, 4, 173), 0.03 + 0.025 * hash(i, 5, 173));
                        (0.5 + a.cos() * d, 0.5 + a.sin() * d, turn, w, h, 0.6 + 0.4 * hash(i, 6, 173))
                    })
                    .collect();
                field(move |x, y| {
                    strips.iter().fold(0.0f32, |acc, &(cx, cy, turn, w, h, ink)| {
                        let (dx, dy) = (x - cx, y - cy);
                        let (s, c) = turn.sin_cos();
                        let (along, across) = (dx * c + dy * s, dy * c - dx * s);
                        acc.max(smooth((w - across.abs()).min(h - along.abs()) * 600.0) * ink)
                    })
                })
            },
        },
        GrayImage {
            name: "Hair",
            pixels: field(|x, y| {
                // a lock of long strands, each waving a little
                let ends = smooth((y - 0.06) * 12.0) * smooth((0.94 - y) * 12.0);
                (0..9).fold(0.0f32, |acc, k| {
                    let cx = 0.22 + 0.56 * (k as f32 + hash(k, 1, 179) * 0.6) / 9.0;
                    let wave = 0.05 * (y * TAU * 1.2 + hash(k, 2, 179) * TAU).sin();
                    acc.max(smooth((0.006 - (x - cx - wave).abs()) * 500.0) * (0.6 + 0.4 * hash(k, 3, 179)))
                }) * ends
            }),
        },
        GrayImage {
            name: "Smudge",
            pixels: field(|x, y| {
                // a soft oval dragged along: streaks run its length
                let (u, v) = centred(x, y);
                let soft = smooth(1.0 - (u * u / 0.5 + v * v / 0.95).sqrt()).powf(0.8);
                soft * (0.6 + 0.4 * value_noise(x * 40.0, y * 3.0, 40, 181))
            }),
        },
        GrayImage {
            name: "Moss",
            pixels: field(|x, y| {
                // clumped fuzz, thick in the middle and sparse at the edge
                let falloff = 1.0 - radius(x, y);
                let n = fbm(x * 16.0, y * 16.0, 16, 3, 191);
                smooth((n - 0.62 + falloff * 0.35) * 8.0) * smooth(falloff * 4.0)
            }),
        },
        GrayImage {
            name: "Heart",
            pixels: {
                let outline = heart();
                field(move |x, y| {
                    let (u, v) = centred(x, y);
                    let (edge, inside) = polygon(&outline, u, v);
                    if inside { smooth(edge * 40.0) } else { 0.0 }
                })
            },
        },
        GrayImage {
            name: "Snowflake",
            pixels: field(|x, y| {
                // six arms, each with two pairs of side branches, round a small hexagon
                let (u, v) = centred(x, y);
                let r = (u * u + v * v).sqrt();
                let sector = TAU / 6.0;
                let a = v.atan2(u).rem_euclid(sector);
                let a = a.min(sector - a);
                let (px, py) = (r * a.cos(), r * a.sin());
                let arm = smooth((0.03 - segment(px, py, (0.0, 0.0), (0.88, 0.0)).0) * 80.0);
                let branch = |at: f32, len: f32| {
                    let tip = (at + len * 0.5, len * 0.866);
                    smooth((0.022 - segment(px, py, (at, 0.0), tip).0) * 80.0)
                };
                let middle = smooth((0.16 - r) * 40.0);
                arm.max(branch(0.38, 0.28)).max(branch(0.6, 0.2)).max(middle)
            }),
        },
        GrayImage {
            name: "Scribble",
            pixels: {
                // a looping line wandering round the middle
                let points: Vec<(f32, f32)> = (0..=96)
                    .map(|i| {
                        let t = i as f32 / 96.0 * TAU * 1.5;
                        (
                            0.5 * (2.1 * t).sin() + 0.25 * (5.3 * t + 1.0).sin(),
                            0.5 * (1.7 * t).cos() + 0.25 * (4.1 * t + 2.0).sin(),
                        )
                    })
                    .collect();
                field(move |x, y| {
                    let (u, v) = centred(x, y);
                    points
                        .windows(2)
                        .filter_map(|w| Some(segment(u, v, *w.first()?, *w.get(1)?).0))
                        .fold(0.0f32, |acc, d| acc.max(smooth((0.02 - d) * 80.0)))
                })
            },
        },
        GrayImage {
            name: "Drip",
            pixels: field(|x, y| {
                // a blob with paint running down from it to a bead
                let (u, v) = centred(x, y);
                let blob = smooth((0.35 - (u * u + (v + 0.55).powi(2)).sqrt()) * 30.0);
                let run = smooth((0.06 - (u - 0.03 * (v * 3.0).sin()).abs()) * 60.0)
                    * f32::from(u8::from(v > -0.6 && v < 0.66));
                let bead = smooth((0.1 - (u * u + (v - 0.7).powi(2)).sqrt()) * 40.0);
                blob.max(run).max(bead)
            }),
        },
        GrayImage {
            name: "Bolt",
            pixels: {
                // a zigzag from the top edge's middle to the bottom's, with two short forks, in a
                // faint glow: laid end to end along a stroke, one bolt runs into the next
                let mut points = vec![(0.0f32, -1.0f32)];
                for k in 1..8 {
                    points.push(((hash(k, 1, 197) - 0.5) * 0.5, -1.0 + k as f32 * 0.25));
                }
                points.push((0.0, 1.0));
                let mut parts: Vec<Line> =
                    points.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?, 0.035))).collect();
                for (k, &from) in points.iter().enumerate().skip(2).step_by(3).take(2) {
                    let side = if from.0 > 0.0 { 1.0 } else { -1.0 };
                    let to = (from.0 + side * (0.25 + 0.15 * hash(k as i32, 2, 197)), from.1 + 0.2);
                    parts.push((from, to, 0.018));
                }
                field(move |x, y| {
                    let (u, v) = centred(x, y);
                    let d = parts.iter().fold(f32::MAX, |acc, &(a, b, w)| acc.min(segment(u, v, a, b).0 - w));
                    smooth(-d * 120.0).max(0.3 * smooth(1.0 - d.max(0.0) * 8.0)) * smooth((1.0 - u.abs()) * 6.0)
                })
            },
        },
        GrayImage {
            name: "Block",
            pixels: field(|x, y| {
                // a solid upright block, half as wide as it is tall, with a few specks out of it
                let (dx, dy) = ((x - 0.5).abs(), (y - 0.47).abs());
                let edge = smooth((0.245 - dx) * 60.0) * smooth((0.4 - dy) * 60.0);
                let n = TEXTURE_SIZE as f32;
                let speck = hash((x * n / 3.0) as i32, (y * n / 3.0) as i32, 1601);
                edge * if speck > 0.86 { 0.25 } else { 1.0 }
            }),
        },
    ];
    // stand-ins for stock shapes: ours drawn smaller in their square, as the stock shapes that come
    // nearest them spread their mass over about half of the square's width, not three quarters
    let shrunk = |index: u32, by: f32, name: &'static str| GrayImage {
        name,
        pixels: shrunk(shapes.get(index as usize).map_or(&[][..], |s| &s.pixels[..]), by),
    };
    let more = [
        shrunk(shape::GRAINY_DISC, SHRINK_DISC, "Grainy dot"),
        shrunk(shape::CHUNK, SHRINK_CHUNK, "Small chunk"),
        shrunk(shape::PASTEL, SHRINK_PASTEL, "Small pastel"),
    ];
    // a solid dab: the small chunk filled in (stock stamped dabs are solid through most of
    // their area: inked mean about 0.85, mostly over 0.9, against the chunk's 0.68)
    let dab = more.get(1).map(|c| c.pixels.iter().map(|&v| (f32::from(v) * DAB_GAIN).min(255.0) as u8).collect());
    shapes.extend(more);
    shapes.push(GrayImage { name: "Solid dab", pixels: dab.unwrap_or_default() });
    shapes
}

/// How much the solid dab raises the small chunk's values.
const DAB_GAIN: f32 = 1.4;

/// How much smaller the stock stand-ins draw our grainy disc, chunk and pastel shapes: as measured,
/// stock brushes on them paint as much of the reference's drawing pad as ours then do (the disc's
/// and the chunk's median areas within 3%; the pastel by the stock shapes' spread alone).
const SHRINK_DISC: f32 = 1.7;
const SHRINK_CHUNK: f32 = 1.47;
const SHRINK_PASTEL: f32 = 1.35;

/// `pixels` (a square image) drawn `by` times smaller about its middle, empty around.
fn shrunk(pixels: &[u8], by: f32) -> Vec<u8> {
    let n = TEXTURE_SIZE;
    let img: Vec<f32> = pixels.iter().map(|&v| f32::from(v)).collect();
    let mut out = vec![0u8; n * n];
    for (k, px) in out.iter_mut().enumerate() {
        let (x, y) = ((k % n) as f32 + 0.5, (k / n) as f32 + 0.5);
        let half = n as f32 / 2.0;
        let (u, v) = (half + (x - half) * by, half + (y - half) * by);
        if u < 0.5 || v < 0.5 || u > n as f32 - 0.5 || v > n as f32 - 0.5 {
            continue;
        }
        *px = sample_wrapped(&img, n, u - 0.5, v - 0.5).round().clamp(0.0, 255.0) as u8;
    }
    out
}

fn make_grains() -> Vec<GrayImage> {
    let mut grains = vec![
        GrayImage { name: "Paper", pixels: field(|x, y| contrast(fbm(x * 32.0, y * 32.0, 32, 4, 101), 1.6)) },
        GrayImage {
            name: "Coarse",
            pixels: field(|x, y| {
                // fine fibres crossing both ways, light with darker threads between (as a charcoal
                // paper's tooth lets paint through)
                let along = fbm_xy(x * 128.0, y * 48.0, (128, 48), 3, 202);
                let down = fbm_xy(x * 48.0, y * 128.0, (48, 128), 3, 203);
                let n = (along + down) / 2.0;
                let dark = ((COARSE_LIGHT - n) * COARSE_DEPTH).max(0.0);
                0.97 - dark
            }),
        },
        GrayImage {
            name: "Weave",
            pixels: field(|x, y| {
                // horizontal and vertical threads, crossing over and under in a checker
                let across = 0.5 + 0.5 * (y * TAU * 16.0).sin();
                let down = 0.5 + 0.5 * (x * TAU * 16.0).sin();
                let over = 0.5 + 0.5 * (x * TAU * 8.0).sin() * (y * TAU * 8.0).sin();
                let n = fbm(x * 64.0, y * 64.0, 64, 2, 303);
                (across + (down - across) * over) * 0.8 + 0.2 * n
            }),
        },
        GrayImage {
            name: "Fibre",
            pixels: field(|x, y| {
                contrast(0.6 * fbm(x * 64.0, y * 4.0, 64, 3, 404) + 0.4 * fbm(x * 4.0, y * 64.0, 4, 3, 505), 1.8)
            }),
        },
        GrayImage {
            name: "Linen",
            pixels: field(|x, y| {
                // uneven threads: their thickness wanders
                let across = value_noise(x * 4.0, y * 4.0, 4, 601);
                let down = value_noise(x * 4.0, y * 4.0, 4, 607);
                let threads = 0.5
                    + 0.25 * ((y * 24.0 + across * 1.5) * std::f32::consts::TAU).sin()
                    + 0.25 * ((x * 24.0 + down * 1.5) * std::f32::consts::TAU).sin();
                contrast(threads * 0.8 + 0.2 * fbm(x * 32.0, y * 32.0, 32, 2, 613), 1.6)
            }),
        },
        GrayImage {
            name: "Wash",
            pixels: field(|x, y| {
                // broad pools of pigment, granulated
                let pools = fbm(x * 4.0, y * 4.0, 4, 3, 701);
                let grains = fbm(x * 64.0, y * 64.0, 64, 2, 709);
                contrast(0.7 * pools + 0.3 * grains, 1.9)
            }),
        },
        GrayImage {
            name: "Concrete",
            pixels: field(|x, y| {
                let surface = fbm(x * 16.0, y * 16.0, 16, 4, 801);
                let pits = fbm(x * 32.0, y * 32.0, 32, 2, 809);
                let pitted = if pits < 0.3 { pits / 0.3 * 0.4 } else { 1.0 };
                contrast(surface, 1.4) * pitted
            }),
        },
        GrayImage {
            name: "Halftone",
            pixels: field(|x, y| {
                // a dot grid, 16 across
                let (fx, fy) = ((x * 16.0).fract() - 0.5, (y * 16.0).fract() - 0.5);
                smooth(((0.36 - (fx * fx + fy * fy).sqrt()) * 30.0).clamp(0.0, 1.0))
            }),
        },
        GrayImage {
            name: "Hatching",
            pixels: field(|x, y| {
                // two sets of diagonal lines, slightly wavering
                let w = (fbm(x * 8.0, y * 8.0, 8, 2, 901) - 0.5) * 0.04;
                let one = 0.5 + 0.5 * ((x + y + w) * std::f32::consts::TAU * 16.0).cos();
                let two = 0.5 + 0.5 * ((x - y + w) * std::f32::consts::TAU * 16.0).cos();
                smooth(((one.max(two) - 0.7) * 6.0).clamp(0.0, 1.0))
            }),
        },
        GrayImage {
            name: "Noise",
            pixels: field(|x, y| {
                // near white, darkened here and there by fine flecks about two texels across: each
                // texel the average of sparse specks over it and its neighbours to the right and
                // below (wrapping, so it tiles), as a speckled image shrunk to half
                let n = TEXTURE_SIZE as i32;
                let (i, j) = ((x * n as f32) as i32, (y * n as f32) as i32);
                let specks: f32 = (0..4)
                    .map(|k| {
                        let u = hash((i + k % 2).rem_euclid(n), (j + k / 2).rem_euclid(n), 1471);
                        ((u - 0.5) * 2.0).max(0.0).powi(2)
                    })
                    .sum();
                1.0 - specks / 4.0
            }),
        },
        GrayImage {
            name: "Crumpled",
            pixels: field(|x, y| {
                // ridges where folds were
                let ridged = 1.0 - (fbm(x * 6.0, y * 6.0, 6, 4, 1103) * 2.0 - 1.0).abs();
                contrast(ridged.powi(2), 1.5)
            }),
        },
        GrayImage {
            name: "Stipple",
            pixels: field(|x, y| {
                // a dot in each of 32 cells across, placed and sized at random
                let (cx, cy) = ((x * 32.0).floor() as i32, (y * 32.0).floor() as i32);
                let (ox, oy) = (0.25 + 0.5 * hash(cx, cy, 1201), 0.25 + 0.5 * hash(cx, cy, 1213));
                let r = 0.2 + 0.2 * hash(cx, cy, 1223);
                let (fx, fy) = ((x * 32.0).fract() - ox, (y * 32.0).fract() - oy);
                smooth(((r - (fx * fx + fy * fy).sqrt()) * 20.0).clamp(0.0, 1.0))
            }),
        },
        GrayImage {
            name: "Wood",
            pixels: field(|x, y| {
                // growth rings running across, bent round knots, fine fibres along them
                let bend = fbm(x * 2.0, y * 2.0, 2, 3, 1301);
                let rings = 0.5 + 0.5 * ((y * 10.0 + bend * 2.0) * TAU).sin();
                let fibres = fbm_xy(x * 4.0, y * 64.0, (4, 64), 2, 1307);
                contrast(0.6 * rings.powf(0.7) + 0.4 * fibres, 1.3)
            }),
        },
        GrayImage {
            name: "Brick",
            pixels: field(|x, y| {
                // eight courses of four bricks, every other one shifted by half; worn faces, deep joints
                let row = (y * 8.0).floor() as i32;
                let shift = 0.5 * row.rem_euclid(2) as f32;
                let (fx, fy) = ((x * 4.0 + shift).fract(), (y * 8.0).fract());
                let joint = (fx.min(1.0 - fx) / 4.0).min(fy.min(1.0 - fy) / 8.0);
                let brick = ((x * 4.0 + shift).floor() as i32).rem_euclid(4);
                let face = 0.62 + 0.18 * hash(brick, row, 1311) + 0.25 * fbm(x * 16.0, y * 16.0, 16, 3, 1313);
                smooth((joint - 0.006) * 300.0) * face.min(1.0)
            }),
        },
        GrayImage {
            name: "Scales",
            pixels: field(|x, y| {
                // rows of round scales, each row lying over the one above, darker at their edges
                let (px, py) = (x * 8.0, y * 16.0);
                let (ci, cj) = (px.floor() as i32, py.floor() as i32);
                let mut best: Option<(i32, f32)> = None;
                for j in cj - 2..=cj + 2 {
                    for i in ci - 1..=ci + 1 {
                        let cx = i as f32 + 0.5 * j.rem_euclid(2) as f32;
                        let d = ((px - cx).powi(2) + ((py - j as f32) * 0.5).powi(2)).sqrt() / 0.62;
                        if d < 1.0 && best.is_none_or(|(bj, _)| j > bj) {
                            best = Some((j, d));
                        }
                    }
                }
                best.map_or(0.0, |(_, d)| 0.3 + 0.7 * smooth((1.0 - d) * 3.0))
            }),
        },
        GrayImage {
            name: "Cells",
            pixels: field(|x, y| {
                let (f1, f2) = cells(x, y, 12, 1321);
                smooth((f2 - f1 - 0.06) * 8.0) * (0.8 + 0.2 * f1)
            }),
        },
        GrayImage {
            name: "Marble",
            pixels: field(|x, y| {
                // thin dark veins wandering through a pale stone
                let warp = fbm(x * 4.0, y * 4.0, 4, 5, 1331);
                let veins = ((x + y + warp * 1.6) * TAU * 2.0).sin().abs();
                veins.powf(0.25) * (0.85 + 0.15 * fbm(x * 32.0, y * 32.0, 32, 2, 1337))
            }),
        },
        GrayImage {
            name: "Stripes",
            pixels: field(|x, y| {
                let wobble = (fbm(x * 4.0, y * 4.0, 4, 2, 1341) - 0.5) * 0.4;
                smooth((0.5 + 0.5 * ((x * 12.0 + wobble) * TAU).cos() - 0.5) * 6.0)
            }),
        },
        GrayImage {
            name: "Grunge",
            pixels: field(|x, y| {
                // blotches, flecks knocked out of them
                let blotch = contrast(fbm(x * 6.0, y * 6.0, 6, 5, 1351), 2.4);
                let flecks = smooth((value_noise(x * 96.0, y * 96.0, 96, 1357) - 0.8) * 10.0);
                (blotch - flecks * 0.8).max(0.0) * (0.8 + 0.2 * fbm(x * 32.0, y * 32.0, 32, 2, 1359))
            }),
        },
        GrayImage {
            name: "Sand",
            pixels: field(|x, y| {
                let fine = value_noise(x * 128.0, y * 128.0, 128, 1361);
                contrast(0.7 * fine + 0.3 * value_noise(x * 64.0, y * 64.0, 64, 1367), 2.6)
            }),
        },
        GrayImage {
            name: "Hexagons",
            pixels: field(|x, y| {
                // a honeycomb's walls: 14 cells across, 16 rows (as near to regular as tiling allows)
                let (cols, rows) = (14.0f32, 16.0f32);
                let (ci, cj) = ((x * cols).floor() as i32, (y * rows).floor() as i32);
                let (mut f1, mut f2) = (f32::MAX, f32::MAX);
                for j in cj - 1..=cj + 1 {
                    for i in ci - 1..=ci + 1 {
                        let cx = (i as f32 + 0.5 + 0.5 * j.rem_euclid(2) as f32) / cols;
                        let d = ((x - cx).powi(2) + (y - (j as f32 + 0.5) / rows).powi(2)).sqrt();
                        (f1, f2) = if d < f1 { (d, f1) } else { (f1, f2.min(d)) };
                    }
                }
                smooth((f2 - f1 - 0.006) * 200.0)
            }),
        },
        GrayImage {
            name: "Caustics",
            pixels: field(|x, y| {
                // light netted on the bottom of a pool: bright where the cells meet
                let wx = (fbm(x * 3.0, y * 3.0, 3, 3, 1381) - 0.5) * 0.15;
                let wy = (fbm(x * 3.0, y * 3.0, 3, 3, 1387) - 0.5) * 0.15;
                let (f1, f2) = cells(x + wx, y + wy, 8, 1391);
                1.0 - smooth((f2 - f1) * 5.0)
            }),
        },
        GrayImage {
            name: "Leather",
            pixels: field(|x, y| {
                let (f1, f2) = cells(x, y, 24, 1401);
                let crease = smooth((f2 - f1) * 6.0) * (0.8 + 0.2 * f1);
                let pores = smooth((value_noise(x * 128.0, y * 128.0, 128, 1409) - 0.75) * 8.0);
                contrast(crease * (0.8 + 0.2 * fbm(x * 8.0, y * 8.0, 8, 2, 1405)) - 0.3 * pores, 1.3)
            }),
        },
        GrayImage {
            name: "Bark",
            pixels: field(|x, y| {
                // deep furrows running up and down
                let ridges = 1.0 - (fbm_xy(x * 16.0, y * 2.0, (16, 2), 4, 1411) * 2.0 - 1.0).abs();
                contrast(ridges.powf(1.5) * (0.8 + 0.2 * fbm(x * 32.0, y * 32.0, 32, 2, 1413)), 1.6)
            }),
        },
        GrayImage {
            name: "Scratches",
            pixels: {
                // fine lines every way across a worn surface
                let lines: Vec<(f32, f32, f32, f32, f32)> = (0..60)
                    .map(|k| {
                        let a = hash(k, 3, 1421) * PI;
                        let len = 0.05 + 0.2 * hash(k, 4, 1421).powi(2);
                        (hash(k, 1, 1421), hash(k, 2, 1421), a.cos() * len, a.sin() * len, 0.6 + 0.4 * hash(k, 5, 1421))
                    })
                    .collect();
                field(move |x, y| {
                    let marks = lines.iter().fold(0.0f32, |acc, &(cx, cy, dx, dy, depth)| {
                        // the nearest copy of the line, the image wrapping round
                        let (u, v) = (x - cx - (x - cx).round(), y - cy - (y - cy).round());
                        acc.max(smooth((0.002 - segment(u, v, (-dx, -dy), (dx, dy)).0) * 400.0) * depth)
                    });
                    (1.0 - 0.9 * marks) * (0.9 + 0.1 * fbm(x * 16.0, y * 16.0, 16, 2, 1427))
                })
            },
        },
        GrayImage {
            name: "Checker",
            pixels: field(|x, y| {
                // shifted by half a square, so its edges fall inside one, not between two
                let (i, j) = ((x * 16.0 + 0.5).floor() as i32, (y * 16.0 + 0.5).floor() as i32);
                f32::from(u8::from((i + j).rem_euclid(2) == 0))
            }),
        },
        GrayImage {
            name: "Speckle",
            pixels: field(|x, y| {
                // a sparse spray of tiny dots
                let (ci, cj) = ((x * 32.0).floor() as i32, (y * 32.0).floor() as i32);
                if hash(ci, cj, 1431) > 0.5 {
                    return 0.0;
                }
                let (ox, oy) = (0.3 + 0.4 * hash(ci, cj, 1433), 0.3 + 0.4 * hash(ci, cj, 1437));
                let (fx, fy) = ((x * 32.0).fract() - ox, (y * 32.0).fract() - oy);
                smooth((0.1 + 0.2 * hash(ci, cj, 1439) - (fx * fx + fy * fy).sqrt()) * 10.0)
            }),
        },
        GrayImage {
            name: "Twill",
            pixels: field(|x, y| {
                // diagonal ribs, each thread showing in short steps
                let rib = 0.5 + 0.5 * ((x + y) * 32.0 * TAU).cos();
                let steps = 0.5 + 0.5 * (x * 64.0 * TAU).cos();
                contrast(0.6 * rib + 0.25 * steps + 0.15 * fbm(x * 32.0, y * 32.0, 32, 2, 1441), 1.5)
            }),
        },
        GrayImage {
            name: "Rust",
            pixels: field(|x, y| {
                // patches eaten away, pitted inside
                let n = fbm(x * 8.0, y * 8.0, 8, 5, 1451);
                let patches = smooth((n - 0.45) * 6.0);
                let pits = smooth((value_noise(x * 64.0, y * 64.0, 64, 1459) - 0.7) * 6.0);
                contrast(0.35 + 0.65 * patches * (1.0 - 0.6 * pits), 1.3)
            }),
        },
        GrayImage { name: "Recycled", pixels: recycled() },
        GrayImage { name: "Paper sheet", pixels: sheet() },
        GrayImage { name: "Worn paper", pixels: worn() },
    ];
    // stand-ins for stock grains: our own patterns with their values spread as measured from
    // brushes drawn with the stock ones
    let respread = |grains: &[GrayImage], from: u32, spread: &[(f32, f32)]| {
        let raw: Vec<f32> =
            grains.get(from as usize).map_or(Vec::new(), |g| g.pixels.iter().map(|&v| f32::from(v)).collect());
        ranked(&raw, spread)
    };
    let charcoal = respread(&grains, grain::COARSE, &CHARCOAL_SPREAD);
    let fine_noise = respread(&grains, grain::NOISE, &FINE_NOISE_SPREAD);
    let speckled = ranked(&soft_noise(1601, 0.4), &SPECKLED_SPREAD);
    grains.push(GrayImage { name: "Charcoal", pixels: charcoal });
    grains.push(GrayImage { name: "Fine noise", pixels: fine_noise });
    grains.push(GrayImage { name: "Speckled", pixels: speckled });
    grains.push(GrayImage { name: "Mottled", pixels: ranked(&soft_noise(4001, 0.4), &MOTTLED_SPREAD) });
    grains
}

/// The charcoal grain's values: most of it from 0.19 to 0.41, a fifth the bright tops of the tooth
/// (as measured from brushes drawn with stock charcoal grains, their stamps building up past it).
const CHARCOAL_SPREAD: [(f32, f32); 9] = [
    (0.0, 0.0),
    (0.188, 0.04),
    (0.225, 0.1),
    (0.262, 0.2),
    (0.3, 0.5),
    (0.338, 0.6),
    (0.413, 0.8),
    (1.0, 0.9),
    (1.0, 1.0),
];

/// The fine noise's values: narrow and dark, from about 0.14 to 0.39 (as measured from a brush
/// drawn with a stock noise grain).
const FINE_NOISE_SPREAD: [(f32, f32); 6] =
    [(0.075, 0.0), (0.141, 0.05), (0.191, 0.25), (0.235, 0.5), (0.279, 0.75), (0.39, 1.0)];

/// The speckled grain's values: a twentieth black, half under 0.09, thinning out up to 0.83. Read
/// between its texels (softened noise, a 230 px tile), it comes out as measured pixel by pixel
/// from a stock speckled grain against how far its stamps build up there (half under 0.095, up
/// to about 0.68), with grain 1.5 px across.
const SPECKLED_SPREAD: [(f32, f32); 15] = [
    (0.0, 0.0),
    (0.003, 0.05),
    (0.012, 0.1),
    (0.03, 0.2),
    (0.048, 0.3),
    (0.068, 0.4),
    (0.09, 0.5),
    (0.117, 0.6),
    (0.147, 0.7),
    (0.195, 0.8),
    (0.292, 0.9),
    (0.395, 0.95),
    (0.506, 0.98),
    (0.655, 0.99),
    (0.825, 1.0),
];

/// The mottled grain's values: mostly light, half over 0.6, from about 0.25 to 0.9 (as a stock
/// grain is applied, typically: the middle of those whose names say nothing of what they are).
const MOTTLED_SPREAD: [(f32, f32); 8] =
    [(0.0, 0.0), (0.25, 0.05), (0.4, 0.2), (0.55, 0.4), (0.65, 0.6), (0.75, 0.8), (0.88, 0.95), (1.0, 1.0)];

/// The paper sheet's values: narrow, nearly all from 0.11 to 0.5, half under 0.29 (a stock paper
/// grain's own values, half as light again, as the stand-ins measured against the reference's pad
/// all come out; narrow, a thin line drawn on it stays smooth).
const SHEET_SPREAD: [(f32, f32); 11] = [
    (0.0, 0.0),
    (0.03, 0.01),
    (0.106, 0.05),
    (0.153, 0.1),
    (0.217, 0.25),
    (0.294, 0.5),
    (0.377, 0.75),
    (0.459, 0.9),
    (0.506, 0.95),
    (0.606, 0.99),
    (0.9, 1.0),
];

/// A paper sheet: fine tooth, each texel its own (as the stock paper's, whose grain is a pixel or
/// two across), its values ranked onto [`SHEET_SPREAD`].
fn sheet() -> Vec<u8> {
    ranked(&soft_noise(3001, 0.0), &SHEET_SPREAD)
}

/// The worn paper's values, as [`RECYCLED_SPREAD`]: light, most of it full and hardly any under
/// 0.33 (as measured from a brush pressed into a stock worn paper).
const WORN_SPREAD: [(f32, f32); 8] =
    [(0.15, 0.0), (0.33, 0.02), (0.48, 0.12), (0.96, 0.3), (1.0, 0.41), (1.0, 0.85), (1.0, 0.99), (1.0, 1.0)];

/// Worn paper: mottled and scuffed, its values ranked onto [`WORN_SPREAD`].
fn worn() -> Vec<u8> {
    let n = TEXTURE_SIZE;
    let raw: Vec<f32> = (0..n * n)
        .map(|k| {
            let (u, v) = (((k % n) as f32 + 0.5) / n as f32, ((k / n) as f32 + 0.5) / n as f32);
            let mottle = fbm(u * 16.0, v * 16.0, 16, 4, 3101);
            let scuffs = value_noise(u * 128.0, v * 32.0, 128, 3109);
            0.6 * mottle + 0.25 * scuffs + 0.15 * value_noise(u * 256.0, v * 256.0, 256, 3119)
        })
        .collect();
    ranked(&raw, &WORN_SPREAD)
}

/// `raw` values ranked onto `spread` ((value, share at or below it), rising): the darkest share
/// of them takes the darkest values, and so on.
fn ranked(raw: &[f32], spread: &[(f32, f32)]) -> Vec<u8> {
    let mut order: Vec<usize> = (0..raw.len()).collect();
    order.sort_by(|&a, &b| raw.get(a).unwrap_or(&0.0).total_cmp(raw.get(b).unwrap_or(&0.0)));
    let mut pixels = vec![0u8; raw.len()];
    let count = order.len().max(1) as f32;
    let last = spread.last().map_or(1.0, |s| s.0);
    for (rank, &k) in order.iter().enumerate() {
        let share = (rank as f32 + 0.5) / count;
        let at = spread.windows(2).find(|w| matches!(w, [_, b] if share <= b.1));
        let value = match at {
            Some([a, b]) => a.0 + (b.0 - a.0) * (share - a.1) / (b.1 - a.1).max(1e-6),
            _ => last,
        };
        if let Some(p) = pixels.get_mut(k) {
            *p = (value * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    pixels
}

/// The recycled paper's values, as a share of the image at or below each (value, share): about
/// an eighth dark fibre, little in between, the sheet itself light, most of it 0.8 and over.
const RECYCLED_SPREAD: [(f32, f32); 11] = [
    (0.0, 0.0),
    (0.075, 0.02),
    (0.225, 0.06),
    (0.33, 0.13),
    (0.48, 0.16),
    (0.81, 0.33),
    (0.96, 0.41),
    (1.0, 0.52),
    (1.0, 0.82),
    (1.0, 0.99),
    (1.0, 1.0),
];

/// Recycled paper: a light, mottled sheet flecked with small dark rough-edged bits, its values
/// ranked onto [`RECYCLED_SPREAD`].
fn recycled() -> Vec<u8> {
    const CELLS: i32 = 32;
    let n = TEXTURE_SIZE;
    // the distance from texel (x, y) to the nearest fleck, in texels: a short segment, at most one
    // to a cell, wrapping so the image tiles
    let cell = n as f32 / CELLS as f32;
    let fibre = |x: f32, y: f32| -> f32 {
        let (ci, cj) = ((x / cell).floor() as i32, (y / cell).floor() as i32);
        let mut nearest = f32::MAX;
        for dj in -1..=1 {
            for di in -1..=1 {
                let (i, j) = (ci + di, cj + dj);
                let (wi, wj) = (i.rem_euclid(CELLS), j.rem_euclid(CELLS));
                if hash(wi, wj, 2011) > 0.62 {
                    continue;
                }
                let centre = ((i as f32 + hash(wi, wj, 2017)) * cell, (j as f32 + hash(wi, wj, 2027)) * cell);
                let turn = hash(wi, wj, 2029) * std::f32::consts::PI;
                let half = 0.3 + 1.7 * hash(wi, wj, 2039);
                let (dx, dy) = (x - centre.0, y - centre.1);
                let (c, s) = (turn.cos(), turn.sin());
                let along = (dx * c + dy * s).clamp(-half, half);
                let (ex, ey) = (dx - along * c, dy - along * s);
                nearest = nearest.min((ex * ex + ey * ey).sqrt());
            }
        }
        nearest
    };
    let raw: Vec<f32> = (0..n * n)
        .map(|k| {
            let (x, y) = ((k % n) as f32 + 0.5, (k / n) as f32 + 0.5);
            let (u, v) = (x / n as f32, y / n as f32);
            let sheet = 0.6 * fbm(u * 16.0, v * 16.0, 16, 4, 2003) + 0.4 * value_noise(u * 128.0, v * 128.0, 128, 2053);
            let rough = 0.8 + 0.9 * value_noise(u * 256.0, v * 256.0, 256, 2063);
            sheet - 1.5 * smooth(1.0 - fibre(x, y) / rough)
        })
        .collect();
    ranked(&raw, &RECYCLED_SPREAD)
}

/// An image from `f(x, y)` with x, y in 0..1 at pixel centres and values clamped to 0..1.
/// Bands of rows are made side by side, one to a core.
fn field(f: impl Fn(f32, f32) -> f32 + Sync) -> Vec<u8> {
    let n = TEXTURE_SIZE;
    let mut out = vec![0u8; n * n];
    let threads = std::thread::available_parallelism().map_or(1, |t| t.get()).clamp(1, 8);
    let rows = n.div_ceil(threads);
    std::thread::scope(|s| {
        for (band, pixels) in out.chunks_mut(rows * n).enumerate() {
            let f = &f;
            s.spawn(move || {
                for (k, px) in pixels.iter_mut().enumerate() {
                    let (i, j) = (k % n, band * rows + k / n);
                    let v = f((i as f32 + 0.5) / n as f32, (j as f32 + 0.5) / n as f32);
                    *px = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                }
            });
        }
    });
    out
}

/// White noise softened over each texel's neighbours by `side` (`side`, 1, `side` each way,
/// wrapping round, so it tiles).
fn soft_noise(seed: u32, side: f32) -> Vec<f32> {
    let n = TEXTURE_SIZE as i32;
    let at = |i: i32, j: i32| hash(i.rem_euclid(n), j.rem_euclid(n), seed);
    let weight = |d: i32| if d == 0 { 1.0 } else { side };
    (0..n * n)
        .map(|k| {
            let (i, j) = (k % n, k / n);
            let mut v = 0.0;
            for dj in -1..=1 {
                for di in -1..=1 {
                    v += weight(di) * weight(dj) * at(i + di, j + dj);
                }
            }
            v
        })
        .collect()
}

/// Distance from the centre, 1 at the inscribed circle.
fn radius(x: f32, y: f32) -> f32 {
    ((x - 0.5).powi(2) + (y - 0.5).powi(2)).sqrt() * 2.0
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn contrast(v: f32, k: f32) -> f32 {
    ((v - 0.5) * k + 0.5).clamp(0.0, 1.0)
}

/// A hash of a lattice point to 0..1.
fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h =
        (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0x00ff_ffff) as f32 / 0x00ff_ffff as f32
}

/// Value noise on a lattice that repeats every `period` cells, so the image tiles.
fn value_noise(x: f32, y: f32, period: i32, seed: u32) -> f32 {
    value_noise_xy(x, y, (period, period), seed)
}

/// Value noise on a lattice that repeats every `period.0` cells across and `period.1` down.
fn value_noise_xy(x: f32, y: f32, period: (i32, i32), seed: u32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (smooth(x - xi as f32), smooth(y - yi as f32));
    let (px, py) = (period.0.max(1), period.1.max(1));
    let at = |i: i32, j: i32| hash(i.rem_euclid(px), j.rem_euclid(py), seed);
    let top = at(xi, yi) + (at(xi + 1, yi) - at(xi, yi)) * fx;
    let bottom = at(xi, yi + 1) + (at(xi + 1, yi + 1) - at(xi, yi + 1)) * fx;
    top + (bottom - top) * fy
}

/// Fractal value noise: `octaves` layers, each at twice the frequency (and period) of the last.
fn fbm(x: f32, y: f32, period: i32, octaves: u32, seed: u32) -> f32 {
    fbm_xy(x, y, (period, period), octaves, seed)
}

/// [`fbm`] repeating every `period.0` cells across and `period.1` down.
fn fbm_xy(x: f32, y: f32, period: (i32, i32), octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut norm, mut f) = (0.0, 0.5, 0.0, 1.0f32);
    for o in 0..octaves {
        let p = (period.0 * f as i32, period.1 * f as i32);
        sum += amp * value_noise_xy(x * f, y * f, p, seed.wrapping_add(o * 7919));
        norm += amp;
        amp *= 0.5;
        f *= 2.0;
    }
    sum / norm
}

/// A heart's outline (in −1..1, point down), from the usual sum of cosines.
fn heart() -> Vec<(f32, f32)> {
    (0..96)
        .map(|i| {
            let t = i as f32 / 96.0 * TAU;
            let x = 16.0 * t.sin().powi(3);
            let y = 13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos();
            (x / 19.0, -(y + 2.5) / 19.0)
        })
        .collect()
}

/// A line drawn in a shape: its two ends and its half width.
type Line = ((f32, f32), (f32, f32), f32);

/// `x`, `y` (0..1) as −1..1 from the centre.
fn centred(x: f32, y: f32) -> (f32, f32) {
    ((x - 0.5) * 2.0, (y - 0.5) * 2.0)
}

/// The distance from (`x`, `y`) to the segment `a`–`b`, and how far along it (0..1) the nearest
/// point is.
fn segment(x: f32, y: f32, a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = (dx * dx + dy * dy).max(1e-12);
    let t = (((x - a.0) * dx + (y - a.1) * dy) / len2).clamp(0.0, 1.0);
    (((x - a.0 - dx * t).powi(2) + (y - a.1 - dy * t).powi(2)).sqrt(), t)
}

/// The distance from (`x`, `y`) to the edge of the polygon through `corners`, and whether the
/// point is inside it.
fn polygon(corners: &[(f32, f32)], x: f32, y: f32) -> (f32, bool) {
    let mut edge = f32::MAX;
    let mut inside = false;
    for (i, &a) in corners.iter().enumerate() {
        let b = corners.get((i + 1) % corners.len()).copied().unwrap_or(a);
        edge = edge.min(segment(x, y, a, b).0);
        if (a.1 > y) != (b.1 > y) && x < (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
    }
    (edge, inside)
}

/// The distances to the nearest and second nearest of points scattered one to a cell, `n` cells
/// across, the cells wrapping round so the image tiles (in cell widths).
fn cells(x: f32, y: f32, n: i32, seed: u32) -> (f32, f32) {
    let (px, py) = (x * n as f32, y * n as f32);
    let (ci, cj) = (px.floor() as i32, py.floor() as i32);
    let (mut f1, mut f2) = (f32::MAX, f32::MAX);
    for j in cj - 1..=cj + 1 {
        for i in ci - 1..=ci + 1 {
            let (wi, wj) = (i.rem_euclid(n), j.rem_euclid(n));
            let (ox, oy) = (hash(wi, wj, seed), hash(wi, wj, seed.wrapping_add(1)));
            let d = ((i as f32 + ox - px).powi(2) + (j as f32 + oy - py).powi(2)).sqrt();
            (f1, f2) = if d < f1 { (d, f1) } else { (f1, f2.min(d)) };
        }
    }
    (f1, f2)
}

/// Cracks spreading from the middle (in −1..1): three jagged lines, each with a few side cracks,
/// as segments (start, end, half width).
fn cracks(seed: u32) -> Vec<Line> {
    /// `steps` steps of a jagged line from `from` (its heading wandering), stopping short of the
    /// edge; where it ended up and its heading there.
    fn walk(
        lines: &mut Vec<Line>,
        from: (f32, f32),
        heading: f32,
        steps: i32,
        width: f32,
        k: i32,
        seed: u32,
    ) -> ((f32, f32), f32) {
        let (mut at, mut a) = (from, heading);
        for s in 0..steps {
            a += hash(k, s, seed) - 0.5;
            let next = (at.0 + a.cos() * 0.15, at.1 + a.sin() * 0.15);
            if (next.0 * next.0 + next.1 * next.1).sqrt() > 0.92 {
                break;
            }
            lines.push((at, next, width * (1.0 - s as f32 / steps as f32 * 0.5)));
            at = next;
        }
        (at, a)
    }
    let mut lines = Vec::new();
    let mut branches = Vec::new();
    for k in 0..3 {
        let (mut at, mut heading) = ((0.0, 0.0), (k as f32 + hash(k, 99, seed) * 0.4) / 3.0 * TAU);
        for part in 0..3 {
            (at, heading) = walk(&mut lines, at, heading, 2, 0.03 - part as f32 * 0.007, k * 10 + part, seed);
            branches.push((at, heading + (hash(k, part + 50, seed) - 0.5) * 2.4));
        }
    }
    for (i, (from, heading)) in branches.into_iter().enumerate() {
        walk(&mut lines, from, heading, 3, 0.012, 100 + i as i32, seed);
    }
    lines
}

/// `count` dots (centre x, centre y, radius) inside the image, deterministic for `seed`.
fn scatter_points(seed: u32, count: i32) -> Vec<(f32, f32, f32)> {
    (0..count)
        .map(|i| {
            let a = hash(i, 1, seed) * std::f32::consts::TAU;
            let d = hash(i, 2, seed).sqrt() * 0.38;
            (0.5 + a.cos() * d, 0.5 + a.sin() * d, 0.02 + 0.07 * hash(i, 3, seed).powi(2))
        })
        .collect()
}

/// How a grain is made to repeat without seams (the grain editor's Auto repeat).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Repeat {
    /// The image this many times across the tile, 1 to 4.
    pub scale: f32,
    /// Turned by this much, radians.
    pub rotation: f32,
    /// How far in from the tile's edges the image is faded into its shifted copy, 0..1 (1: to
    /// the middle).
    pub overlap: f32,
    /// How sharp that fade is, 0 (soft) to 1 (hard).
    pub hardness: f32,
    /// Fade into the shifted copy turned round, not just shifted.
    pub mirror: bool,
    /// Fade the fine detail over a narrower band than the broad tones.
    pub pyramid: bool,
}

/// `pixels` (a `TEXTURE_SIZE` square greyscale image; anything else comes back unchanged) made to
/// tile without seams as `repeat` says: faded at its edges into a copy shifted by half a tile,
/// which runs on across them, then scaled and turned (wrapping round) and faded again.
pub fn repeatable(pixels: &[u8], repeat: Repeat) -> Vec<u8> {
    let n = TEXTURE_SIZE;
    if pixels.len() != n * n {
        return pixels.to_vec();
    }
    let img: Vec<f32> = pixels.iter().map(|&v| f32::from(v)).collect();
    let seamless = blend_edges(&img, repeat);
    let scale = if repeat.scale.is_finite() { repeat.scale.clamp(1.0, 4.0) } else { 1.0 };
    let rotation = if repeat.rotation.is_finite() { repeat.rotation } else { 0.0 };
    let out = if (scale - 1.0).abs() < 1e-4 && rotation.abs() < 1e-4 {
        seamless
    } else {
        // the seamless image read through the scale and turn, wrapping round, its edges faded again
        let (sin, cos) = rotation.sin_cos();
        let half = n as f32 / 2.0;
        let mut moved = vec![0.0; n * n];
        for y in 0..n {
            for x in 0..n {
                let (dx, dy) = (x as f32 + 0.5 - half, y as f32 + 0.5 - half);
                let u = (dx * cos + dy * sin) * scale + half;
                let v = (-dx * sin + dy * cos) * scale + half;
                if let Some(px) = moved.get_mut(y * n + x) {
                    *px = sample_wrapped(&seamless, n, u - 0.5, v - 0.5);
                }
            }
        }
        blend_edges(&moved, repeat)
    };
    out.iter().map(|&v| v.round().clamp(0.0, 255.0) as u8).collect()
}

/// `img` faded at its edges into its copy shifted by half a tile (turned round too, with
/// mirror), over a band `overlap` of the way to the middle.
fn blend_edges(img: &[f32], repeat: Repeat) -> Vec<f32> {
    let n = TEXTURE_SIZE;
    let half = n / 2;
    let at = |x: usize, y: usize| img.get(y * n + x).copied().unwrap_or(0.0);
    let other: Vec<f32> = (0..n * n)
        .map(|i| {
            let (x, y) = ((i % n + half) % n, (i / n + half) % n);
            if repeat.mirror { at(n - 1 - x, n - 1 - y) } else { at(x, y) }
        })
        .collect();
    let overlap = if repeat.overlap.is_finite() { repeat.overlap.clamp(0.0, 1.0) } else { 1.0 };
    let width = overlap * half as f32;
    if width < 1.0 {
        return img.to_vec();
    }
    let hardness = if repeat.hardness.is_finite() { repeat.hardness.clamp(0.0, 1.0) } else { 0.0 };
    // how much of the image itself shows at (x, y): none at the edges, all a band in
    let mask = |x: usize, y: usize, band: f32| {
        let d = x.min(n - 1 - x).min(y).min(n - 1 - y) as f32 + 0.5;
        let t = (d / band.max(1.0)).clamp(0.0, 1.0);
        smooth((t - 0.5) * (1.0 + hardness * 8.0) + 0.5)
    };
    let mix = |a: &[f32], b: &[f32], band: f32| -> Vec<f32> {
        (0..n * n)
            .map(|i| {
                let m = mask(i % n, i / n, band);
                let (p, q) = (a.get(i).copied().unwrap_or(0.0), b.get(i).copied().unwrap_or(0.0));
                p * m + q * (1.0 - m)
            })
            .collect()
    };
    if !repeat.pyramid {
        return mix(img, &other, width);
    }
    // broad tones over the whole band, fine detail over a third of it
    let (low_a, low_b) = (blurred(img, n / 32), blurred(&other, n / 32));
    let high = |v: &[f32], low: &[f32]| v.iter().zip(low).map(|(a, b)| a - b).collect::<Vec<f32>>();
    let tones = mix(&low_a, &low_b, width);
    let detail = mix(&high(img, &low_a), &high(&other, &low_b), width / 3.0);
    tones.iter().zip(&detail).map(|(a, b)| a + b).collect()
}

/// A box blur of radius `r` over the square image, wrapping round its edges.
fn blurred(img: &[f32], r: usize) -> Vec<f32> {
    let n = TEXTURE_SIZE;
    let span = (2 * r + 1) as f32;
    let pass = |src: &[f32], across: bool| -> Vec<f32> {
        let at = |a: usize, b: usize| {
            let (x, y) = if across { (a % n, b) } else { (b, a % n) };
            src.get(y * n + x).copied().unwrap_or(0.0)
        };
        let mut out = vec![0.0; n * n];
        for b in 0..n {
            for a in 0..n {
                let sum: f32 = (0..=2 * r).map(|k| at(a + n - r + k, b)).sum();
                let (x, y) = if across { (a, b) } else { (b, a) };
                if let Some(px) = out.get_mut(y * n + x) {
                    *px = sum / span;
                }
            }
        }
        out
    };
    pass(&pass(img, true), false)
}

/// The image at (`u`, `v`) (pixel corners at whole numbers), bilinear, wrapping round.
fn sample_wrapped(img: &[f32], n: usize, u: f32, v: f32) -> f32 {
    let (fx, fy) = (u.floor(), v.floor());
    let (tx, ty) = (u - fx, v - fy);
    let wrap = |c: f32| (c as i64).rem_euclid(n as i64) as usize;
    let at = |x: f32, y: f32| img.get(wrap(y) * n + wrap(x)).copied().unwrap_or(0.0);
    let top = at(fx, fy) * (1.0 - tx) + at(fx + 1.0, fy) * tx;
    let bottom = at(fx, fy + 1.0) * (1.0 - tx) + at(fx + 1.0, fy + 1.0) * tx;
    top * (1.0 - ty) + bottom * ty
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_have_the_right_size_and_content() {
        for img in shapes().into_iter().chain(grains()) {
            assert_eq!(img.pixels.len(), TEXTURE_SIZE * TEXTURE_SIZE, "{}", img.name);
            let max = *img.pixels.iter().max().unwrap();
            let min = *img.pixels.iter().min().unwrap();
            // (a stand-in spread as measured may stay dark, as the fine noise does)
            assert!(max >= min + 48, "{} has contrast", img.name);
        }
    }

    #[test]
    fn shapes_are_empty_at_the_corners() {
        for img in shapes() {
            assert!(img.pixels[0] < 8, "{} corner {}", img.name, img.pixels[0]);
        }
    }

    #[test]
    fn grains_tile() {
        // the left column continues from the right one as any column does from its neighbour
        for img in grains() {
            let n = TEXTURE_SIZE;
            let step = |a: usize, b: usize| -> u32 {
                (0..n).map(|j| u32::from(img.pixels[j * n + a].abs_diff(img.pixels[j * n + b]))).sum::<u32>() / n as u32
            };
            let inside = (1..n).map(|i| step(i - 1, i)).sum::<u32>() / (n as u32 - 1);
            let seam = step(n - 1, 0);
            assert!(seam <= inside * 3 / 2 + 6, "{} seam {seam}, inside {inside}", img.name);
        }
    }

    /// Writes every shape and grain as a PGM into the directory in `PROCRAFT_DUMP`, for looking at
    /// them: `cargo test -p procraft-engine dump_textures -- --ignored`.
    #[test]
    #[ignore]
    fn dump_textures() {
        let Ok(dir) = std::env::var("PROCRAFT_DUMP") else { return };
        for (kind, images) in [("shape", shapes()), ("grain", grains())] {
            for (i, img) in images.iter().enumerate() {
                let mut pgm = format!(
                    "P5 {TEXTURE_SIZE} {TEXTURE_SIZE} 255
"
                )
                .into_bytes();
                pgm.extend(&img.pixels);
                std::fs::write(format!("{dir}/{kind}-{i:02}-{}.pgm", img.name.replace(' ', "_")), pgm).unwrap();
            }
        }
    }

    #[test]
    fn generation_is_deterministic() {
        assert_eq!(shapes(), shapes());
        assert_eq!(grains(), grains());
    }

    #[test]
    fn a_repeated_grain_has_no_seams() {
        let n = TEXTURE_SIZE;
        // a ramp: a hard edge where it wraps round
        let ramp: Vec<u8> = (0..n * n).map(|i| (i % n * 255 / (n - 1)) as u8).collect();
        let jump =
            |img: &[u8]| (0..n).map(|y| (i32::from(img[y * n]) - i32::from(img[y * n + n - 1])).abs()).max().unwrap();
        assert!(jump(&ramp) > 200);
        let base = Repeat { scale: 1.0, rotation: 0.0, overlap: 1.0, hardness: 0.0, mirror: false, pyramid: false };
        for r in [
            base,
            Repeat { mirror: true, ..base },
            Repeat { pyramid: true, ..base },
            Repeat { scale: 2.0, rotation: 0.6, ..base },
        ] {
            let out = repeatable(&ramp, r);
            assert!(jump(&out) <= 6, "{r:?}: {}", jump(&out));
        }
        // no overlap: left as it was
        assert_eq!(repeatable(&ramp, Repeat { overlap: 0.0, ..base }), ramp);
        assert_eq!(repeatable(&[1, 2, 3], base), vec![1, 2, 3]);
    }
}
