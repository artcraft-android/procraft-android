import SwiftUI

private let sliderLeft: CGFloat = 5
private let sizeTrackTop: CGFloat = 6
private let opacityTrackTop: CGFloat = 248

/// A button lights up fast and goes back more slowly.
private func highlight(_ on: Bool) -> Animation {
    .linear(duration: on ? 0.067 : 0.15)
}

/// The bar across the top of the canvas: artwork actions on the left, painting tools on the right.
struct TopBar: View {
    let model: CanvasModel

    var body: some View {
        GeometryReader { bar in
            let gaps = Pc.barGaps(bar.size.width)
            // the layers and colour panels take the highlight from the current tool while open
            let panelOpen = model.panel == .layers || model.panel == .color || model.transforming
                || model.selecting || model.adjusting
            ZStack {
                HStack(spacing: gaps.action) {
                    Text("Gallery")
                        .font(.system(size: 18, weight: .medium))
                        .foregroundStyle(Pc.barText)
                        .lineLimit(1)
                        .fixedSize()
                        .offset(y: -1)
                        .frame(width: Pc.galleryWidth, height: Pc.barButtonHeight, alignment: .leading)
                        .contentShape(Rectangle())
                        .onTapGesture { model.onClose() }
                    ActionButton(icon: PcIcons.actions, selected: model.panel == .actions) { model.toggle(.actions) }
                    ActionButton(icon: PcIcons.adjustments, selected: model.panel == .adjustments || model.adjusting || model.recoloring) {
                        model.toggle(.adjustments)
                    }
                    ActionButton(icon: PcIcons.selection, selected: model.selecting, glyph: 33) {
                        model.toggleSelecting()
                    }
                    ActionButton(icon: PcIcons.transform, selected: model.transforming, glyph: 44) {
                        model.toggleTransform()
                    }
                }
                .padding(.leading, Pc.barMargin)
                .frame(maxWidth: .infinity, alignment: .leading)
                HStack(spacing: gaps.tool) {
                    ToolButton(icon: PcIcons.brush, selected: !panelOpen && model.tool == .paint, onHold: { model.toolHeld(.paint) }) {
                        model.toolPressed(.paint)
                    }
                    ToolButton(icon: PcIcons.smudge, selected: !panelOpen && model.tool == .smudge, onHold: { model.toolHeld(.smudge) }) {
                        model.toolPressed(.smudge)
                    }
                    ToolButton(icon: PcIcons.erase, selected: !panelOpen && model.tool == .erase, onHold: { model.toolHeld(.erase) }) {
                        model.toolPressed(.erase)
                    }
                    ToolButton(icon: PcIcons.layers, selected: model.panel == .layers) { model.toggle(.layers) }
                    ColorButton(model: model)
                }
                .padding(.trailing, Pc.barMargin)
                .frame(maxWidth: .infinity, alignment: .trailing)
            }
            .frame(height: Pc.barHeight)
        }
        .frame(height: Pc.barHeight)
        .background(Pc.bar)
    }
}

/// An action button: its glyph on a dark disc, which turns blue while its panel is open.
private struct ActionButton: View {
    let icon: PcIcon
    let selected: Bool
    var glyph: CGFloat = 36
    let action: () -> Void

    var body: some View {
        ZStack {
            Circle()
                .fill(selected ? Pc.barGlyphSelected : Pc.actionDisc)
                .frame(width: Pc.actionDiscSize, height: Pc.actionDiscSize)
            // the glyphs are drawn in a 44 frame, shown `glyph` across (most 36)
            PcIconView(icon, size: glyph, color: selected ? .white : Pc.actionGlyph)
        }
        .animation(highlight(selected), value: selected)
        .frame(width: Pc.barButtonWidth, height: Pc.barButtonHeight)
        .contentShape(Rectangle())
        .onTapGesture(perform: action)
    }
}

/// A tool button: tapped, the tool (or its brushes); held (`onHold`), it takes the current brush.
private struct ToolButton: View {
    let icon: PcIcon
    let selected: Bool
    var onHold: (() -> Void)?
    let action: () -> Void

    var body: some View {
        // a 44 frame shown as it is, over the 38 tall button
        let button = PcIconView(icon, size: 44, color: selected ? Pc.barGlyphSelected : Pc.barGlyph)
            .animation(highlight(selected), value: selected)
            .frame(width: Pc.barButtonWidth, height: Pc.barButtonHeight)
            .contentShape(Rectangle())
            .onTapGesture(perform: action)
        if let onHold {
            button.onLongPressGesture(perform: onHold)
        } else {
            button
        }
    }
}

/// The colour button: the colour painted with, ringed only where it would fade into the bar.
private struct ColorButton: View {
    let model: CanvasModel

    /// The brightness (HSB) under which it is ringed on the dark bar.
    private static let darkRingBelow: Float = 0.35
    /// The brightness over which it is ringed on the light bar.
    private static let lightRingAbove: Float = 0.85

    var body: some View {
        let ringed = Look.shared.light ? model.brightness > Self.lightRingAbove : model.brightness < Self.darkRingBelow
        Circle()
            .fill(CanvasSpace.shared.color(model.colorRgb()))
            .overlay {
                if ringed {
                    Circle().strokeBorder(Pc.colorRing, lineWidth: 1)
                }
            }
            .frame(width: Pc.colorDot, height: Pc.colorDot)
            .frame(width: Pc.barButtonWidth, height: Pc.barButtonHeight)
            // held, the colour painted with before; dragged out onto the canvas, it fills there
            .overlay {
                DropSource(model: model, onHold: { model.previousColor() }, onTap: { model.toggle(.color) })
            }
    }
}

/// The side bar against the left edge (the right one, `right`-handed): brush size, the modify
/// button, opacity, undo and redo. On the right everything is mirrored but the glyphs.
struct SideBar: View {
    let model: CanvasModel
    var right = false

    /// A control's left edge, mirrored on the right.
    private func x(_ left: CGFloat, _ width: CGFloat) -> CGFloat {
        right ? Pc.sideBarWidth - left - width : left
    }

    var body: some View {
        let fill = Pc.sideBar
        ZStack(alignment: .topLeading) {
            // translucent, with windows where the slider tracks are: they show what is behind
            Canvas { context, size in
                let r = Pc.sideBarCorner
                var shape = Path()
                shape.addRoundedRect(
                    in: CGRect(origin: .zero, size: size),
                    cornerRadii: right
                        ? RectangleCornerRadii(topLeading: r, bottomLeading: r, bottomTrailing: 0, topTrailing: 0)
                        : RectangleCornerRadii(topLeading: 0, bottomLeading: 0, bottomTrailing: r, topTrailing: r),
                    style: .circular
                )
                for top in [sizeTrackTop, opacityTrackTop] {
                    let track = CGRect(x: x(sliderLeft, Pc.sliderWidth), y: top, width: Pc.sliderWidth, height: Pc.sliderHeight)
                    shape.addRoundedRect(in: track, cornerSize: CGSize(width: Pc.sliderCorner, height: Pc.sliderCorner), style: .circular)
                }
                context.fill(shape, with: .color(fill), style: FillStyle(eoFill: true))
            }
            .frame(width: Pc.sideBarWidth, height: Pc.sideBarHeight)
            .allowsHitTesting(false)

            BrushSlider(
                value: model.size,
                notches: model.sizeNotches,
                panel: hooks(.size),
                onChange: model.changeSize
            )
            .offset(x: x(sliderLeft, Pc.sliderWidth), y: sizeTrackTop)

            ModifyButton(model: model)
                .offset(x: x(5, 40), y: 191)

            BrushSlider(
                value: model.opacity,
                notches: model.opacityNotches,
                panel: hooks(.opacity),
                onChange: model.changeOpacity
            )
            .offset(x: x(sliderLeft, Pc.sliderWidth), y: opacityTrackTop)

            SideButton(icon: PcIcons.undo, enabled: model.canUndo) { model.undo() }
                .offset(x: x(5, 40), y: 430.5)
            SideButton(icon: PcIcons.redo, enabled: model.canRedo) { model.redo() }
                .offset(x: x(5, 40), y: 472.5)

            // beside the slider it belongs to, a little above it, outside the bar
            if let which = model.sliderPanel {
                let top = which == .size ? sizeTrackTop : opacityTrackTop
                SliderPanelView(model: model, which: which)
                    .offset(x: x(sliderLeft + Pc.sliderWidth + sliderPanelGap, sliderPanelSide), y: top - sliderPanelRise)
            }
        }
        .frame(width: Pc.sideBarWidth, height: Pc.sideBarHeight, alignment: .topLeading)
    }

    private func hooks(_ which: SliderPanel) -> SliderPanelHooks {
        SliderPanelHooks(
            show: { held in
                model.sliderPanel = which
                model.sliderPanelKept = held
            },
            hide: {
                if !model.sliderPanelKept {
                    model.sliderPanel = nil
                }
            }
        )
    }
}

/// The modify button: held, it changes what a canvas touch does; tapped, it does what Tap □ is
/// set to (at first: the next touch on the canvas picks a colour).
private struct ModifyButton: View {
    let model: CanvasModel
    @State private var down = false

    var body: some View {
        RoundedRectangle(cornerRadius: Pc.modifyCorner - Pc.modifyStroke / 2, style: .circular)
            .stroke(model.pickNext ? Pc.accent : Pc.sideGlyph, lineWidth: Pc.modifyStroke)
            .padding(Pc.modifyStroke / 2)
            .frame(width: Pc.modifySize, height: Pc.modifySize)
            .frame(width: 40, height: 41)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { _ in
                        if !down {
                            down = true
                            model.modifyPressed()
                        }
                    }
                    .onEnded { value in
                        down = false
                        let inside = CGRect(x: 0, y: 0, width: 40, height: 41).contains(value.location)
                        model.modifyReleased(cancelled: !inside)
                    }
            )
    }
}

private struct SideButton: View {
    let icon: PcIcon
    let enabled: Bool
    let action: () -> Void

    var body: some View {
        // a 50 frame shown 36 across
        PcIconView(icon, size: 36, color: enabled ? Pc.sideGlyph : Pc.sideGlyphDisabled)
            .frame(width: 40, height: 41)
            .contentShape(Rectangle())
            .onTapGesture {
                if enabled {
                    action()
                }
            }
    }
}

/// What a slider's panel does: shown while it is dragged (`held` false), kept up when it is held
/// still.
struct SliderPanelHooks {
    let show: (_ held: Bool) -> Void
    let hide: () -> Void
}

private let sliderPanelSide: CGFloat = 190
private let sliderPanelGap: CGFloat = 8
private let sliderPanelRise: CGFloat = 10
// dark glass over the canvas (what is behind it blurred), or light
private var sliderPanelFill: Color { Look.shared.light ? Color(argb: 0xFFE7_E7E7) : Color(argb: 0xC93E_3E3E) }
private var sliderPanelInk: Color { Look.shared.light ? Color(argb: 0xFF39_3939) : Color(argb: 0xFFF0_F0F0) }
// the dot is drawn in the interface's ink, not the brush's colour
private var sliderPanelDot: Color { Look.shared.light ? .black : .white }

/// A slider's panel: its name and value ("Size 8%"), a button saving the value as a notch on the
/// slider ("+"; "-" takes away the notch it is on), and in the middle the brush's shape (a dot
/// for a round tip) as it would paint now, its size on the screen (and at its opacity).
private struct SliderPanelView: View {
    let model: CanvasModel
    let which: SliderPanel
    var body: some View {
        let size = which == .size
        let value = size ? model.size : model.opacity
        // read again as the size changes
        let diameter = CGFloat(model.brushScreenDiameter()) / model.pixels
        let shape = model.brushOutline()
        let onNotch = model.notchAt(size: size, value: value) != nil
        let ink = sliderPanelDot.opacity(size ? 1 : Double(min(max(value, 0.02), 1)))
        let shape22 = RoundedRectangle(cornerRadius: 22, style: .circular)
        ZStack(alignment: .topLeading) {
            Canvas { context, area in
                let centre = CGPoint(x: area.width / 2, y: area.height / 2)
                let r = min(max(diameter / 2, 0.75), min(area.width, area.height) / 2 - 8)
                if shape.isEmpty {
                    context.fill(Path(ellipseIn: CGRect(x: centre.x - r, y: centre.y - r, width: r * 2, height: r * 2)), with: .color(ink))
                } else {
                    // its loops across the diameter, a hole where one lies inside another
                    var path = Path()
                    for ring in shape {
                        for i in 0 ..< ring.count / 2 {
                            let p = CGPoint(x: centre.x + CGFloat(ring[i * 2]) * r * 2, y: centre.y + CGFloat(ring[i * 2 + 1]) * r * 2)
                            if i == 0 {
                                path.move(to: p)
                            } else {
                                path.addLine(to: p)
                            }
                        }
                        path.closeSubpath()
                    }
                    context.fill(path, with: .color(ink), style: FillStyle(eoFill: true))
                }
            }
            (Text(verbatim: size ? "Size" : "Opacity").bold() + Text(verbatim: " \(Int((value * 100).rounded()))%"))
                .font(.system(size: 19))
                .foregroundStyle(sliderPanelInk)
                .lineLimit(1)
                .fixedSize()
                .frame(height: 32)
                .offset(x: 16, y: 6)
            if onNotch || model.canSaveNotch(size: size) {
                PcIconView(onNotch ? PcIcons.notchRemove : PcIcons.notchAdd, size: 24, color: sliderPanelInk)
                    .frame(width: 32, height: 32)
                    .contentShape(Rectangle())
                    .onTapGesture { model.toggleNotch(size: size) }
                    .offset(x: 150, y: 6)
            }
        }
        .frame(width: sliderPanelSide, height: sliderPanelSide, alignment: .topLeading)
        .background {
            if Look.shared.light {
                shape22.fill(sliderPanelFill)
            } else {
                shape22.fill(.ultraThinMaterial).overlay(shape22.fill(sliderPanelFill))
            }
        }
        // a hairline rim, as measured
        .overlay(shape22.strokeBorder(Pc.glassMenuEdge, lineWidth: 1))
        .clipShape(shape22)
    }
}

/// Where a slider's touch has got to.
private final class SliderTouch {
    var began = false
    var lastY: CGFloat = 0
    /// Where the finger has taken the value, before a notch pulls it in.
    var raw: Float = 0
    var moved = false
    var held = false
    var hold: DispatchWorkItem?
}

/// An upright slider moved by dragging anywhere on it: the thumb follows the finger (relative,
/// not jump-to-touch), 1 at the top. Moved, its `panel` shows while it moves; held still, it
/// stays up. Its `notches` (saved values) show as marks on the track; a value dragged near one is
/// pulled onto it, and a tap on one goes there. On a notch, the thumb shows a blue bar.
struct BrushSlider: View {
    let value: Float
    var notches: [Float] = []
    var panel: SliderPanelHooks?
    let onChange: (Float) -> Void

    @State private var touch = SliderTouch()

    private static let notchWidth: CGFloat = 20
    private static let notchHeight: CGFloat = 4
    private static let notchMark = Color(argb: 0x1AFF_FFFF)
    /// A touch held this long without moving keeps the panel up (seconds).
    private static let holdSeconds = 0.5

    private var travel: CGFloat {
        Pc.sliderHeight - Pc.thumbHeight - Pc.thumbInset * 2
    }

    private func near(_ v: Float, reach: Float = CanvasModel.notchNear) -> Float? {
        notches.filter { abs($0 - v) < reach }.min { abs($0 - v) < abs($1 - v) }
    }

    var body: some View {
        let track = Pc.sliderTrack
        let rimColour = Pc.sliderRim
        let thumbColour = Pc.sliderThumb
        let accent = Pc.accent
        Canvas { context, size in
            let inset = Pc.thumbInset
            let thumb = CGSize(width: Pc.thumbWidth, height: Pc.thumbHeight)
            let travel = size.height - thumb.height - inset * 2
            let at = CGPoint(x: (size.width - thumb.width) / 2, y: inset + travel * CGFloat(1 - value))
            let knobRect = CGRect(origin: at, size: thumb)
            let knob = Path(roundedRect: knobRect, cornerRadius: thumb.height / 2, style: .circular)
            // the track has a window where the thumb is, so the thumb shows over the bar alone; a
            // lighter rim runs round it
            let corner = Pc.sliderCorner
            let rim = Pc.sliderRimWidth
            let inside = Path(roundedRect: CGRect(x: rim, y: rim, width: size.width - rim * 2, height: size.height - rim * 2), cornerRadius: corner - rim, style: .circular)
            var window = inside
            window.addPath(knob)
            context.fill(window, with: .color(track), style: FillStyle(eoFill: true))
            var ring = Path(roundedRect: CGRect(origin: .zero, size: size), cornerRadius: corner, style: .circular)
            ring.addPath(inside)
            context.fill(ring, with: .color(rimColour), style: FillStyle(eoFill: true))
            // the notches: short bars across the track; on one, the thumb shows a blue bar
            let bar = CGSize(width: Self.notchWidth, height: Self.notchHeight)
            for n in notches {
                let y = inset + thumb.height / 2 + travel * CGFloat(1 - n)
                let mark = CGRect(x: (size.width - bar.width) / 2, y: y - bar.height / 2, width: bar.width, height: bar.height)
                context.fill(Path(roundedRect: mark, cornerRadius: bar.height / 2, style: .circular), with: .color(Self.notchMark))
            }
            context.fill(knob, with: .color(thumbColour))
            if notches.contains(where: { abs($0 - value) < 1e-4 }) {
                let mark = CGRect(x: (size.width - bar.width) / 2, y: at.y + (thumb.height - bar.height) / 2, width: bar.width, height: bar.height)
                context.fill(Path(roundedRect: mark, cornerRadius: bar.height / 2, style: .circular), with: .color(accent))
            }
        }
        .frame(width: Pc.sliderWidth, height: Pc.sliderHeight)
        .contentShape(Rectangle())
        .gesture(
            DragGesture(minimumDistance: 0)
                .onChanged { drag in
                    let t = touch
                    if !t.began {
                        t.began = true
                        t.lastY = drag.startLocation.y
                        t.raw = value
                        t.moved = false
                        t.held = false
                        let hold = DispatchWorkItem { [panel] in
                            if t.began, !t.moved {
                                t.held = true
                                panel?.show(true)
                            }
                        }
                        t.hold = hold
                        DispatchQueue.main.asyncAfter(deadline: .now() + Self.holdSeconds, execute: hold)
                    }
                    let dy = drag.location.y - t.lastY
                    if dy != 0 {
                        if !t.moved {
                            panel?.show(false)
                        }
                        t.moved = true
                        t.lastY = drag.location.y
                        t.raw = min(max(t.raw - Float(dy / travel), 0), 1)
                        onChange(near(t.raw) ?? t.raw)
                    }
                }
                .onEnded { drag in
                    let t = touch
                    t.hold?.cancel()
                    t.hold = nil
                    // a tap on a notch's mark goes to it
                    if !t.moved, !t.held {
                        let at = 1 - Float((drag.startLocation.y - Pc.thumbInset - Pc.thumbHeight / 2) / travel)
                        if let notch = near(at, reach: Float(Pc.thumbHeight / 2 / travel)) {
                            onChange(notch)
                        }
                    }
                    if t.moved {
                        panel?.hide()
                    }
                    t.began = false
                }
        )
    }
}
