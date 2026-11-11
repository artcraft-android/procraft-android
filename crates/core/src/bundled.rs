//! The plugins that come inside the app, for now: each the packed file of its folder under
//! `plugins/`, kept in `crates/core/bundled-plugins/` (`python tools/pack_plugin.py --bundle`
//! writes them). [`crate::Plugins::with_bundled`] adds each to the store the first time the app
//! comes with it, as if imported but off, and from then on it is like any imported plugin. To ship
//! the app without them, this file (its test with it), its `mod` line and that folder go, and
//! `with_bundled` adds none: what was added stays, as imported.

/// Each by its folder's name under `plugins/`, and the file packed from it.
pub(crate) const BUNDLED: &[(&str, &[u8])] = &[
    ("brush-sounds", include_bytes!("../bundled-plugins/brush-sounds.procraft-plugin")),
    ("colour-triangle", include_bytes!("../bundled-plugins/colour-triangle.procraft-plugin")),
    ("extract-lines", include_bytes!("../bundled-plugins/extract-lines.procraft-plugin")),
    ("eyedropper-layer", include_bytes!("../bundled-plugins/eyedropper-layer.procraft-plugin")),
    ("fill-all-layers", include_bytes!("../bundled-plugins/fill-all-layers.procraft-plugin")),
    ("gap-closing", include_bytes!("../bundled-plugins/gap-closing.procraft-plugin")),
    ("grid-layer", include_bytes!("../bundled-plugins/grid-layer.procraft-plugin")),
    ("hide-undo-notice", include_bytes!("../bundled-plugins/hide-undo-notice.procraft-plugin")),
    ("interface-colour", include_bytes!("../bundled-plugins/interface-colour.procraft-plugin")),
    ("interface-size", include_bytes!("../bundled-plugins/interface-size.procraft-plugin")),
    ("opacity-together", include_bytes!("../bundled-plugins/opacity-together.procraft-plugin")),
    ("outline", include_bytes!("../bundled-plugins/outline.procraft-plugin")),
    ("paper-texture", include_bytes!("../bundled-plugins/paper-texture.procraft-plugin")),
    ("paste-into-masks", include_bytes!("../bundled-plugins/paste-into-masks.procraft-plugin")),
    ("pin-panels", include_bytes!("../bundled-plugins/pin-panels.procraft-plugin")),
    ("posterise", include_bytes!("../bundled-plugins/posterise.procraft-plugin")),
    ("quickshape-curves", include_bytes!("../bundled-plugins/quickshape-curves.procraft-plugin")),
    ("radial-segments", include_bytes!("../bundled-plugins/radial-segments.procraft-plugin")),
    ("randomise-brush", include_bytes!("../bundled-plugins/randomise-brush.procraft-plugin")),
    ("touch-offset", include_bytes!("../bundled-plugins/touch-offset.procraft-plugin")),
    ("tweening", include_bytes!("../bundled-plugins/tweening.procraft-plugin")),
    ("vector-canvas", include_bytes!("../bundled-plugins/vector-canvas.procraft-plugin")),
];

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use serde_json::Value;

    use super::*;
    use crate::plugin_file::{read_package, sha256};
    use crate::plugins::PluginPart;

    /// What to do when one of these fails.
    const REPACK: &str = "re-pack them: python tools/pack_plugin.py --bundle";

    fn plugins_folder() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins")
    }

    /// The paths under `folder`, `prefix` before each, as `tools/pack_plugin.py` walks them.
    fn paths(folder: &Path, prefix: &str, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(folder).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().into_string().unwrap();
            if name.starts_with('.') {
                continue;
            }
            if entry.file_type().unwrap().is_dir() {
                if name != "target" {
                    paths(&entry.path(), &format!("{prefix}{name}/"), found);
                }
            } else {
                found.push(format!("{prefix}{name}"));
            }
        }
    }

    /// `folder` as `tools/pack_plugin.py` digests it: each file's path, size and bytes, in order.
    fn digest(folder: &Path) -> String {
        let mut found = Vec::new();
        paths(folder, "", &mut found);
        found.sort();
        let mut all = Vec::new();
        for path in found {
            let data = std::fs::read(folder.join(&path)).unwrap();
            all.extend_from_slice(format!("{path}\n{}\n", data.len()).as_bytes());
            all.extend_from_slice(&data);
        }
        sha256(&all)
    }

    /// What a plugin's packed file is made from, as `tools/pack_plugin.py` digests it: its folder,
    /// and the plugin kit too when it is built with it.
    fn source_digest(folder: &Path) -> String {
        let mine = digest(folder);
        let cargo = std::fs::read_to_string(folder.join("Cargo.toml")).unwrap_or_default();
        if cargo.contains("path = \"../kit\"") {
            sha256(format!("{mine}{}", digest(&plugins_folder().join("kit"))).as_bytes())
        } else {
            mine
        }
    }

    #[test]
    fn each_holds_up_as_a_plugin_file_and_is_its_folder_as_it_is_now() {
        let root = plugins_folder();
        let sources = Path::new(env!("CARGO_MANIFEST_DIR")).join("bundled-plugins/sources.txt");
        let sources = std::fs::read_to_string(sources).unwrap();
        let mut ids = Vec::new();
        for (name, bytes) in BUNDLED {
            let package = read_package(bytes).unwrap_or_else(|refusal| panic!("{name}: {refusal:?}"));
            let folder = root.join(name);
            // the manifest as packed is the folder's, with the files it lists added
            let text = std::fs::read(folder.join("craft-extension.json")).unwrap();
            let wanted: Value = serde_json::from_slice(&text).unwrap();
            let mut packed: Value = serde_json::from_slice(package.manifest_text()).unwrap();
            packed.as_object_mut().unwrap().remove("files");
            assert_eq!(packed, wanted, "{name}'s manifest has changed: {REPACK}");
            // and so is a filter's code
            let filter = folder.join("filter.wgsl");
            if package.manifest.holds.contains(&PluginPart::Filter) {
                let wanted = std::fs::read(filter).unwrap();
                assert_eq!(package.file("filter.wgsl"), Some(wanted.as_slice()), "{name}'s filter: {REPACK}");
            } else {
                assert!(!filter.exists(), "{name}");
            }
            // code with hooks, but for a plugin of switches (no crate, its hooks answered from its
            // controls)
            let code = package.file(crate::plugin_run::MODULE).is_some();
            let crate_ = folder.join("Cargo.toml").exists();
            assert_eq!(code, package.manifest.holds.contains(&PluginPart::Hooks) && crate_, "{name}");
            assert!(code || !crate_, "{name} has a crate but no code");
            // its code can't be built here, so the folder as it was packed, all of it, is checked
            let line = format!("{name} {}", source_digest(&folder));
            assert!(sources.lines().any(|kept| kept == line), "plugins/{name} has changed: {REPACK}");
            ids.push(package.manifest.id);
        }
        let mut distinct = ids.clone();
        distinct.sort();
        distinct.dedup();
        assert_eq!(distinct.len(), ids.len(), "{ids:?}");
        // every folder under plugins/ comes with the app, and nothing else does
        let mut folders: Vec<String> = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| root.join(name).join("craft-extension.json").is_file())
            .collect();
        folders.sort();
        let names: Vec<&str> = BUNDLED.iter().map(|(name, _)| *name).collect();
        assert_eq!(folders, names, "{REPACK}, and list each here");
        assert_eq!(sources.lines().count(), names.len());
    }
}
