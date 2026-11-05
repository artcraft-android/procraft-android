import SwiftUI

/// How a slider reads. `size` reads the size the control sets as a percent (Max at its end), `both`
/// goes both ways from 0% in its middle, -100% to Max (and `follow` and `grade` to a name of their
/// own), `double` reads 100% at its middle; `metal` and `gloss` name their ends as a material's
/// metalness and roughness.
enum Shown {
    case percent, both, count, tip, movement, zoom, charge, size, follow, grade, double, twice, metal, gloss
}

/// What a control waits on: another control's key and the choice it must be on; greyed out
/// otherwise.
struct EnabledBy {
    let key: String
    let choice: Int
}

/// A choice among kinds shown as an icon and a name.
struct StudioKind {
    let icon: PcIcon
    let name: String
}

/// One control in the brush studio.
indirect enum StudioControl {
    case slider(key: String, label: String, shown: Shown, enabledBy: EnabledBy?)
    case toggle(key: String, label: String, enabledBy: EnabledBy?)
    case choice(key: String, options: [String])
    case kinds(key: String, options: [StudioKind])
    /// The taper's two ends on a bar, with the switch linking their sizes under it.
    case taperBar(start: String, end: String, link: String)
    case pressureCurve
    /// The tilt angle on its quarter-circle graph.
    case tiltAngle
    /// The shape's roundness and angle on an ellipse with handles.
    case roundness
    /// The brush's kind of source image, opening its editor.
    case source(label: String, kind: SourceKind)
    /// A blend mode, kept by the engine's number for it, picked from a list.
    case picker(key: String, label: String)
    /// A card that a tap on its title opens to show its controls inside it.
    case disclosure(label: String, controls: [StudioControl])
    case previewCard
    case about
}

struct StudioSection {
    let title: String?
    let controls: [StudioControl]
}

/// A page of the studio: its name, its icon (shown `iconSize` across) and its sections.
struct StudioTab {
    let name: String
    let icon: PcIcon
    let iconSize: CGFloat
    let sections: [StudioSection]
}

private func slider(_ key: String, _ label: String, _ shown: Shown = .percent, enabledBy: EnabledBy? = nil) -> StudioControl {
    .slider(key: key, label: label, shown: shown, enabledBy: enabledBy)
}

private func toggle(_ key: String, _ label: String, enabledBy: EnabledBy? = nil) -> StudioControl {
    .toggle(key: key, label: label, enabledBy: enabledBy)
}

private func section(_ title: String?, _ controls: [StudioControl]) -> StudioSection {
    StudioSection(title: title, controls: controls)
}

private let filtering: [String] = ["No filtering", "Classic filtering", "Improved filtering"]

/// A colour's hue, saturation and brightness sliders, then its secondary colour's.
private func colorJitter(_ prefix: String, both: Bool, secondary: String) -> [StudioControl] {
    let shown: Shown = both ? .both : .percent
    return [
        slider("\(prefix)_hue", "Hue", shown),
        slider("\(prefix)_saturation", "Saturation", shown),
        slider("\(prefix)_brightness", "Brightness", shown),
        slider(secondary, "Secondary Color"),
    ]
}

/// The studio's tabs, in order, with what each shows.
let studioTabs: [StudioTab] = [
    strokePathTab, stabilizationTab, taperTab, shapeTab, grainTab, renderingTab, wetMixTab, colorDynamicsTab,
    dynamicsTab, stylusTab, propertiesTab, materialsTab, previewTab, aboutTab,
]

private let strokePathTab = StudioTab(name: "Stroke path", icon: PcIcons.studioStroke, iconSize: 33, sections: [
    section("Stroke properties", [
        slider("spacing", "Spacing"), slider("spacing_jitter", "Spacing Jitter"), slider("jitter_lateral", "Jitter Lateral"),
        slider("jitter_linear", "Jitter Linear"), slider("falloff", "Fall off"),
    ]),
])

private let stabilizationTab = StudioTab(name: "Stabilization", icon: PcIcons.studioStabilize, iconSize: 45, sections: [
    section("StreamLine", [slider("streamline", "Amount"), slider("streamline_pressure", "Pressure")]),
    section("Stabilization", [slider("stabilization", "Amount")]),
    section("Motion filtering", [slider("motion_filtering", "Amount"), slider("motion_expression", "Expression")]),
])

private let taperTab = StudioTab(name: "Taper", icon: PcIcons.studioTaper, iconSize: 41, sections: [
    section("Pressure taper", [
        .taperBar(start: "pen_taper_start", end: "pen_taper_end", link: "pen_taper_link"),
        slider("pen_taper_size", "Size"), slider("pen_taper_opacity", "Opacity"), slider("pen_taper_pressure", "Pressure"),
        slider("pen_taper_tip", "Tip", .tip), toggle("pen_tip_animation", "Tip animation"),
    ]),
    section("Touch taper", [
        .taperBar(start: "touch_taper_start", end: "touch_taper_end", link: "touch_taper_link"),
        slider("touch_taper_size", "Size"), slider("touch_taper_opacity", "Opacity"), slider("touch_taper_tip", "Tip", .tip),
    ]),
    section("Taper properties", [toggle("classic_taper", "Classic taper")]),
])

private let shapeTab = StudioTab(name: "Shape", icon: PcIcons.studioShape, iconSize: 45, sections: [
    section(nil, [.source(label: "Shape Source", kind: .shape)]),
    section("Input style", [
        .choice(key: "input_style", options: ["Touch only", "Azimuth", "Azimuth and barrel roll"]),
        // only with the barrel roll to follow
        toggle("relative_to_stroke", "Relative to stroke", enabledBy: EnabledBy(key: "input_style", choice: 2)),
    ]),
    section("Touch properties", [slider("shape_rotation", "Rotation", .follow)]),
    section("Shape properties", [
        slider("scatter", "Scatter", .twice), slider("count", "Count", .count), slider("count_jitter", "Count jitter"),
        toggle("randomized", "Randomized"), toggle("flip_x", "Flip X"), toggle("flip_y", "Flip Y"),
        .roundness, slider("pressure_roundness", "Pressure Roundness", .both),
        slider("tilt_roundness", "Tilt roundness", .both), slider("roundness_jitter_y", "Roundness Vertical Jitter"),
        slider("roundness_jitter_x", "Roundness Horizontal Jitter"),
    ]),
    section("Shape filtering", [.choice(key: "shape_filtering", options: filtering)]),
])

private let grainTab = StudioTab(name: "Grain", icon: PcIcons.studioGrain, iconSize: 45, sections: [
    section(nil, [.source(label: "Grain Source", kind: .grain)]),
    section("Grain behavior", [
        .kinds(key: "grain_behavior", options: [
            StudioKind(icon: PcIcons.grainMoving, name: "Moving"), StudioKind(icon: PcIcons.grainTexturized, name: "Texturized"),
        ]),
        slider("grain_movement", "Movement", .movement), slider("grain_scale", "Scale"), slider("grain_zoom", "Zoom", .zoom),
        slider("grain_rotation", "Rotation", .follow), slider("grain_depth", "Depth"), slider("grain_depth_min", "Depth minimum"),
        slider("grain_depth_jitter", "Depth jitter"), toggle("grain_offset_jitter", "Offset jitter"),
        .picker(key: "grain_blend", label: "Blend mode"), slider("grain_brightness", "Brightness", .both),
        slider("grain_contrast", "Contrast", .both),
    ]),
    section("Grain filtering", [.choice(key: "grain_filtering", options: filtering)]),
    section("3D grain behaviour", [toggle("grain_follows_camera", "Grain follows camera")]),
])

private let renderingTab = StudioTab(name: "Rendering", icon: PcIcons.studioRendering, iconSize: 37, sections: [
    section("Rendering mode", [
        .choice(key: "rendering_mode", options: ["Light Glaze", "Uniformed Glaze", "Intense Glaze", "Heavy Glaze", "Uniform Blending", "Intense Blending"]),
    ]),
    section("Blending", [
        slider("flow", "Flow"), slider("wet_edges", "Wet edges"), slider("burnt_edges", "Burnt edges"),
        .picker(key: "burnt_blend", label: "Burnt edges mode"), .picker(key: "blend_mode", label: "Blend mode"),
        toggle("luminance_blending", "Luminance blending"), toggle("alpha_threshold", "Alpha Threshold"),
        slider("alpha_threshold_amount", "Threshold Amount", enabledBy: EnabledBy(key: "alpha_threshold", choice: 1)),
        // only for a brush paired with another, which none are yet
        toggle("classic_combine", "Classic Normal Combine mode", enabledBy: EnabledBy(key: "dual_brush", choice: 1)),
    ]),
])

private let wetMixTab = StudioTab(name: "Wet Mix", icon: PcIcons.studioWet, iconSize: 45, sections: [
    section("Wet mix", [
        slider("wet_dilution", "Dilution"), slider("wet_charge", "Charge", .charge), slider("wet_attack", "Attack"),
        slider("wet_pull", "Pull"), slider("wet_grade", "Grade", .grade), slider("wet_blur", "Blur"),
        slider("wet_blur_jitter", "Blur jitter"), slider("wet_jitter", "Wetness jitter"),
    ]),
])

private let colorDynamicsTab = StudioTab(name: "Color dynamics", icon: PcIcons.studioColor, iconSize: 41, sections: [
    section("Stamp color jitter", [
        slider("stamp_hue", "Hue"), slider("stamp_saturation", "Saturation"), slider("stamp_lightness", "Lightness"),
        slider("stamp_darkness", "Darkness"), slider("stamp_secondary", "Secondary color"),
    ]),
    section("Stroke color jitter", [
        slider("stroke_hue", "Hue"), slider("stroke_saturation", "Saturation"), slider("stroke_lightness", "Lightness"),
        slider("stroke_darkness", "Darkness"), slider("stroke_secondary", "Secondary color"),
    ]),
    section("Color pressure", colorJitter("pressure", both: true, secondary: "pressure_secondary")),
    section("Color tilt", colorJitter("tilt", both: true, secondary: "tilt_secondary")),
    section("Color barrel roll", colorJitter("roll", both: false, secondary: "roll_secondary")),
])

private let dynamicsTab = StudioTab(name: "Dynamics", icon: PcIcons.studioDynamics, iconSize: 37, sections: [
    section("Speed", [slider("speed_size", "Size", .both), slider("speed_opacity", "Opacity", .both), slider("speed_spacing", "Spacing")]),
    section("Jitter", [slider("jitter_size", "Size"), slider("jitter_opacity", "Opacity")]),
])

private let stylusTab = StudioTab(name: "Stylus", icon: PcIcons.studioStylus, iconSize: 43, sections: [
    section("Pressure", [
        .pressureCurve, slider("pressure_size", "Size", .both), slider("pressure_flow", "Opacity"),
        slider("pressure_opacity", "Flow", .both), slider("pressure_bleed", "Bleed"),
    ]),
    section("Tilt", [
        .tiltAngle, slider("tilt_opacity", "Opacity"), slider("tilt_gradation", "Gradation"),
        slider("tilt_bleed", "Bleed"), slider("tilt_size", "Size"), toggle("tilt_size_compression", "Size compression"),
    ]),
    section("Barrel roll", [slider("roll_size", "Size", .both), slider("roll_opacity", "Opacity"), slider("roll_bleed", "Bleed")]),
    section("Cursor outline", [.choice(key: "cursor_outline", options: ["None", "Contrast", "Active color"])]),
    section("Hover", [slider("hover_pressure", "Estimated pressure")]),
    section("Hover fill", [.choice(key: "hover_fill", options: ["None", "Shape", "All"])]),
])

private let propertiesTab = StudioTab(name: "Properties", icon: PcIcons.studioProperties, iconSize: 49, sections: [
    section("Brush properties", [toggle("orient_to_screen", "Orient to screen"), slider("smudge_pull", "Smudge Pull")]),
    section("Brush behavior", [
        slider("max_size", "Maximum size", .size), slider("min_size", "Minimum size", .size),
        slider("max_opacity", "Maximum opacity"), slider("min_opacity", "Minimum opacity"),
    ]),
])

private let materialsTab = StudioTab(name: "Materials", icon: PcIcons.studioMaterials, iconSize: 29, sections: [
    section("3D Materials", [
        .disclosure(label: "Metallic", controls: [
            slider("metallic_amount", "Amount", .metal), .source(label: "Metallic Source", kind: .metallic),
            slider("metallic_scale", "Scale"),
        ]),
        .disclosure(label: "Roughness", controls: [
            slider("roughness_amount", "Amount", .gloss), .source(label: "Roughness Source", kind: .roughness),
            slider("roughness_scale", "Scale"),
        ]),
    ]),
])

private let previewTab = StudioTab(name: "Preview", icon: PcIcons.studioPreview, iconSize: 45, sections: [
    section("Preview render", [.previewCard]),
    section("Preview properties", [
        toggle("preview_stamp", "Use stamp preview"), slider("preview_size", "Size"), slider("preview_pressure_min", "Pressure Minimum"),
        slider("preview_pressure_scale", "Pressure Scale", .double), toggle("preview_wet_mix", "Wet Mix"),
        slider("preview_tilt_offset", "Tilt angle", .both),
    ]),
])

private let aboutTab = StudioTab(name: "About this brush", icon: PcIcons.studioAbout, iconSize: 45, sections: [
    section(nil, [.about]),
])

/// Every key the studio's controls read.
let studioKeys: [String] = {
    var keys: [String] = []
    func add(_ key: String) {
        if !key.isEmpty, !keys.contains(key) {
            keys.append(key)
        }
    }
    func read(_ control: StudioControl) {
        switch control {
        case let .slider(key, _, _, _):
            // a slider's own tilt switch and angle, which its keypad shows
            let extras = sliderExtras(key: key)
            add(key)
            add(extras.tiltSwitch)
            add(extras.tiltAngle)
        case let .toggle(key, _, _):
            add(key)
        case let .choice(key, _):
            add(key)
        case let .kinds(key, _):
            add(key)
        case let .picker(key, _):
            add(key)
        case let .taperBar(start, end, link):
            add(start)
            add(end)
            add(link)
        case .tiltAngle:
            add("tilt_angle")
        case .roundness:
            add("roundness")
            add("shape_angle")
        case let .disclosure(_, controls):
            controls.forEach(read)
        case .pressureCurve, .source, .previewCard, .about:
            break
        }
    }
    for tab in studioTabs {
        for section in tab.sections {
            section.controls.forEach(read)
        }
    }
    add("most_size")
    return keys
}()
