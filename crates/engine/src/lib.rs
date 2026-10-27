//! ProCraft's painting engine: the document model, brushes and stroke geometry, and the canvas
//! view. No GPU and no platform code: `procraft-gpu` draws what this crate describes, and the app
//! feeds it input.

pub mod abr;
pub mod animation;
pub mod applz4;
pub mod base64;
pub mod brush;
pub mod brushfile;
pub mod cms;
pub mod cmyk;
pub mod crop;
pub mod curve;
#[cfg(test)]
mod damaged;
pub mod document;
pub mod environment;
pub mod fill;
pub mod formats;
pub mod geom;
pub mod gltf;
pub mod guide;
pub mod icc;
pub mod input;
pub mod jpeg;
pub mod keyed;
pub mod library;
pub mod mesh;
pub mod motion;
pub mod native;
pub mod plist;
pub mod preview;
pub mod profile;
pub mod psd;
pub mod quickshape;
pub mod selection;
pub mod stroke;
pub mod studio;
pub mod textures;
pub mod tone;
pub mod transform;
pub mod usd;
pub mod view;
pub mod warp;
pub mod zip;
