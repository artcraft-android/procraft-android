//! Grid layer: its action adds a layer named "Grid" over the current one, with lines across and
//! down it in the brush's colour, as far apart and as wide as its sliders say. One undo takes it
//! all back. It's a plugin, not part of the app: `tools/pack_plugin.py` makes a `.procraft-plugin`
//! file of it.
//!
//! It shows a plugin's action: what it may reach (the canvas's size, adding and painting a layer,
//! the brush's colour) its manifest asks for, and the plugin kit's `app` calls it.

use kit::app;

/// Its sliders: how far apart the lines are, and how wide, pixels.
static CONTROLS: kit::Controls<2> = kit::Controls::new([100.0, 2.0]);
kit::controls!(CONTROLS);
kit::actions!(|_| add_grid());

fn add_grid() {
    let (width, height) = app::canvas_size();
    if width == 0 || height == 0 {
        return;
    }
    let Some(layer) = app::add_layer() else {
        app::say("There's no room for another layer.");
        return;
    };
    app::name_layer(layer, "Grid");
    let spacing = (CONTROLS.get(0).round() as u32).max(2);
    let line = (CONTROLS.get(1).round() as u32).clamp(1, spacing - 1);
    let rgb = app::brush_colour();
    let ink = [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255];
    // each line a strip of its own: the lines down, then across
    let strip = |w: u32, h: u32| ink.repeat(w as usize * h as usize);
    for x in (0..width).step_by(spacing as usize) {
        let w = line.min(width - x);
        app::paint(layer, x as i32, 0, w, height, &strip(w, height));
    }
    for y in (0..height).step_by(spacing as usize) {
        let h = line.min(height - y);
        app::paint(layer, 0, y as i32, width, h, &strip(width, h));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn with_no_artwork_it_does_nothing() {
        super::add_grid();
    }
}
