//! A plugin's own panel (interface 2, the `panel` permission): rows it describes as data, which each
//! app draws as its own cards are drawn, so a plugin's panel looks like the app's. An action shows it
//! (`show_panel` with the rows as JSON text); each touch on a row goes back to the plugin as
//! `procraft_panel_event(row, value)`, in the same copy of its code, which may then show the panel
//! again changed, change the artwork as an action does, or close it.
//!
//! ```json
//! { "title": "Halftone", "rows": [
//!   { "type": "slider", "label": "Dot size", "least": 1, "most": 50, "step": 1, "value": 8, "unit": " px" },
//!   { "type": "switch", "label": "Round dots", "on": true },
//!   { "type": "choice", "label": "Shape", "options": ["Dots", "Lines"], "chosen": 0 },
//!   { "type": "text", "text": "Works on the current layer." },
//!   { "type": "button", "label": "Apply" }
//! ] }
//! ```
//!
//! Rows go by their place in `rows`; a value is a slider's, a switch's (1 on, 0 off), a choice's
//! place, or 1 for a button tapped.

use serde::Deserialize;

/// The most rows a panel may have, and the most choices one row may give.
const MOST_ROWS: usize = 16;
const MOST_OPTIONS: usize = 8;

/// A plugin's panel as the apps show it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PluginPanel {
    pub title: String,
    pub rows: Vec<PanelRow>,
}

/// One of its rows.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum PanelRow {
    /// Words, a line or a few.
    Text {
        text: String,
    },
    /// A slider from `least` to `most` in steps of `step`, `unit` after its value.
    Slider {
        label: String,
        least: f32,
        most: f32,
        step: f32,
        value: f32,
        unit: String,
    },
    Switch {
        label: String,
        on: bool,
    },
    /// One of `options`, the one at `chosen` picked.
    Choice {
        label: String,
        options: Vec<String>,
        chosen: u32,
    },
    /// A row tapped, its words in the accent colour.
    Button {
        label: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPanel {
    title: String,
    rows: Vec<RawRow>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
enum RawRow {
    Text {
        text: String,
    },
    Slider {
        label: String,
        least: f32,
        most: f32,
        #[serde(default)]
        step: Option<f32>,
        value: f32,
        #[serde(default)]
        unit: String,
    },
    Switch {
        label: String,
        #[serde(default)]
        on: bool,
    },
    Choice {
        label: String,
        options: Vec<String>,
        #[serde(default)]
        chosen: u32,
    },
    Button {
        label: String,
    },
}

/// Words that show as they are: some, not too many, and no control characters.
fn plain(text: &str, most: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= most && !text.chars().any(char::is_control)
}

/// The panel `json` describes, if it holds up: a title, 1 to 16 rows, each with words that show
/// and numbers that make sense. A slider's value is put in its range.
pub(crate) fn read_panel(json: &str) -> Option<PluginPanel> {
    let raw: RawPanel = serde_json::from_str(json).ok()?;
    if !plain(&raw.title, 40) || raw.rows.is_empty() || raw.rows.len() > MOST_ROWS {
        return None;
    }
    let rows = raw.rows.into_iter().map(read_row).collect::<Option<Vec<_>>>()?;
    Some(PluginPanel { title: raw.title, rows })
}

fn read_row(raw: RawRow) -> Option<PanelRow> {
    Some(match raw {
        RawRow::Text { text } => {
            // a few lines: line breaks are the only control characters it may have
            let shows = !text.trim().is_empty()
                && text.chars().count() <= 400
                && !text.chars().any(|c| c.is_control() && c != '\n');
            shows.then_some(PanelRow::Text { text })?
        }
        RawRow::Slider { label, least, most, step, value, unit } => {
            let step = step.unwrap_or(0.0);
            let numbers = [least, most, step, value].iter().all(|n| n.is_finite());
            if !plain(&label, 40) || !numbers || least >= most || step < 0.0 || unit.chars().count() > 8 {
                return None;
            }
            let value = value.clamp(least, most);
            PanelRow::Slider { label, least, most, step, value, unit }
        }
        RawRow::Switch { label, on } => plain(&label, 40).then_some(PanelRow::Switch { label, on })?,
        RawRow::Choice { label, options, chosen } => {
            let fits = plain(&label, 40)
                && (1..=MOST_OPTIONS).contains(&options.len())
                && options.iter().all(|o| plain(o, 24))
                && (chosen as usize) < options.len();
            fits.then_some(PanelRow::Choice { label, options, chosen })?
        }
        RawRow::Button { label } => plain(&label, 40).then_some(PanelRow::Button { label })?,
    })
}

/// The value row `row` of `panel` takes when touched with `value`, as the plugin is told it: a
/// slider's in its range and on its steps, a switch's 1 or 0, a choice's place, a button's 1. None
/// for a row that takes no touch or isn't there, or a value that is no number.
pub(crate) fn touched(panel: &PluginPanel, row: usize, value: f32) -> Option<f32> {
    if !value.is_finite() {
        return None;
    }
    match panel.rows.get(row)? {
        PanelRow::Text { .. } => None,
        PanelRow::Slider { least, most, step, .. } => {
            let on_step = if *step > 0.0 { least + ((value - least) / step).round() * step } else { value };
            Some(on_step.clamp(*least, *most))
        }
        PanelRow::Switch { .. } => Some(if value >= 0.5 { 1.0 } else { 0.0 }),
        PanelRow::Choice { options, .. } => {
            let at = value.round();
            (at >= 0.0 && (at as usize) < options.len()).then_some(at)
        }
        PanelRow::Button { .. } => Some(1.0),
    }
}

/// `panel` with row `row` showing `value` (as [`touched`] gave it), so it shows the touch at once,
/// before the plugin answers.
pub(crate) fn with_value(mut panel: PluginPanel, row: usize, value: f32) -> PluginPanel {
    match panel.rows.get_mut(row) {
        Some(PanelRow::Slider { value: shown, .. }) => *shown = value,
        Some(PanelRow::Switch { on, .. }) => *on = value >= 0.5,
        Some(PanelRow::Choice { chosen, .. }) => *chosen = value as u32,
        _ => {}
    }
    panel
}

#[cfg(test)]
mod tests {
    use super::*;

    const HALFTONE: &str = r#"{ "title": "Halftone", "rows": [
        { "type": "slider", "label": "Dot size", "least": 1, "most": 50, "step": 1, "value": 80, "unit": " px" },
        { "type": "switch", "label": "Round dots", "on": true },
        { "type": "choice", "label": "Shape", "options": ["Dots", "Lines"], "chosen": 1 },
        { "type": "text", "text": "Works on the current layer.\nOne undo takes it back." },
        { "type": "button", "label": "Apply" }
    ] }"#;

    #[test]
    fn a_panel_is_read_its_numbers_put_right() {
        let panel = read_panel(HALFTONE).unwrap();
        assert_eq!(panel.title, "Halftone");
        assert_eq!(panel.rows.len(), 5);
        // a value past the slider's end shows at its end
        assert!(matches!(panel.rows[0], PanelRow::Slider { value, .. } if value == 50.0));
        assert_eq!(
            panel.rows[2],
            PanelRow::Choice { label: "Shape".into(), options: vec!["Dots".into(), "Lines".into()], chosen: 1 }
        );
        assert_eq!(panel.rows[4], PanelRow::Button { label: "Apply".into() });
    }

    #[test]
    fn a_panel_that_doesn_t_hold_up_is_none() {
        let one = |row: &str| read_panel(&format!(r#"{{ "title": "T", "rows": [{row}] }}"#));
        assert!(one(r#"{ "type": "button", "label": "Go" }"#).is_some());
        // no rows, too many, a kind this app doesn't have, a field it doesn't know
        assert!(read_panel(r#"{ "title": "T", "rows": [] }"#).is_none());
        let many = vec![r#"{ "type": "button", "label": "Go" }"#; 17].join(",");
        assert!(read_panel(&format!(r#"{{ "title": "T", "rows": [{many}] }}"#)).is_none());
        assert!(one(r#"{ "type": "web", "label": "Go" }"#).is_none());
        assert!(one(r#"{ "type": "button", "label": "Go", "script": "x" }"#).is_none());
        // words that don't show, numbers that make no sense
        assert!(one(r#"{ "type": "button", "label": " " }"#).is_none());
        assert!(one(r#"{ "type": "switch", "label": "a\u0007b" }"#).is_none());
        assert!(one(r#"{ "type": "slider", "label": "S", "least": 5, "most": 5, "value": 5 }"#).is_none());
        assert!(one(r#"{ "type": "slider", "label": "S", "least": 0, "most": 1, "step": -1, "value": 0 }"#).is_none());
        assert!(one(r#"{ "type": "choice", "label": "C", "options": [], "chosen": 0 }"#).is_none());
        assert!(one(r#"{ "type": "choice", "label": "C", "options": ["A"], "chosen": 1 }"#).is_none());
        assert!(read_panel(r#"{ "title": "", "rows": [{ "type": "button", "label": "Go" }] }"#).is_none());
        assert!(read_panel("not json").is_none());
    }

    #[test]
    fn a_touch_is_told_as_its_row_takes_it() {
        let panel = read_panel(HALFTONE).unwrap();
        // on the slider's steps and in its range
        assert_eq!(touched(&panel, 0, 7.4), Some(7.0));
        assert_eq!(touched(&panel, 0, 99.0), Some(50.0));
        assert_eq!(touched(&panel, 1, 0.2), Some(0.0));
        assert_eq!(touched(&panel, 2, 1.0), Some(1.0));
        assert_eq!(touched(&panel, 2, 2.0), None, "no third shape");
        assert_eq!(touched(&panel, 3, 1.0), None, "words take no touch");
        assert_eq!(touched(&panel, 4, 0.0), Some(1.0));
        assert_eq!(touched(&panel, 9, 1.0), None);
        assert_eq!(touched(&panel, 0, f32::NAN), None);
        // and shows at once
        let shown = with_value(panel, 1, 0.0);
        assert_eq!(shown.rows[1], PanelRow::Switch { label: "Round dots".into(), on: false });
    }
}
