import SwiftUI
import UIKit
import UniformTypeIdentifiers

private func themed(_ dark: UInt32, _ light: UInt32) -> Color {
    Color(argb: Look.shared.light ? light : dark)
}

private var screenFill: Color { themed(0xFF1F_1F1F, 0xFFF2_F2F2) }
private var panelFill: Color { themed(0xFF28_2828, 0xFFDF_DFDF) }
private var ink: Color { themed(0xFFAA_AAAA, 0xFF3E_3E3E) }
private var idleInk: Color { themed(0xFF79_7979, 0xFF3E_3E3E) }
private let editing = Color(argb: 0xFF43_6BF0)
private var keypadFill: Color { themed(0xFF1A_1A1A, 0xFFD1_D3D8) }
private var digitFill: Color { themed(0xFF52_5252, 0xFFFF_FFFF) }
private var keyFill: Color { themed(0xFF2B_2B2B, 0xFFAB_B2BB) }
private var unitChosen: Color { themed(0xFFA1_A1A1, 0xFF81_8C96) }
private var switchTrack: Color { themed(0xFF35_3535, 0xFFD5_D5D5) }
private var switchIdle: Color { themed(0xFFC1_C1C1, 0xFF3B_3B3B) }
private var capsuleFill: Color { themed(0xFF1A_1A1A, 0xFFFB_FBFB) }
private var capsuleEdge: Color { themed(0xFF37_3737, 0xFFE0_E0E0) }
private let capsuleInk = Color(argb: 0xFFA5_A5A5)
private let doneBlue = Color(argb: 0xFF01_90FF)
/// Where the last row the keypad types into (DPI) ends, down from the top: the page's 120, two rows
/// and their line, 24 between the groups, one row.
private let typedBottom: CGFloat = 286

/// The units sizes are typed in: the key's name, the unit's (as presets keep it) and its mark.
private enum SizeUnit: CaseIterable {
    case millimeters, centimeters, inches, pixels

    var label: String {
        switch self {
        case .millimeters: "Millimeters"
        case .centimeters: "Centimeters"
        case .inches: "Inches"
        case .pixels: "Pixels"
        }
    }

    var key: String {
        switch self {
        case .millimeters: "mm"
        case .centimeters: "cm"
        case .inches: "in"
        case .pixels: "px"
        }
    }

    var mark: String {
        self == .inches ? "″" : key
    }
}

/// The fields the keypad types into.
private enum Field {
    case width, height, dpi
}

private let sections: [(label: String, icon: PcIcon, size: CGFloat)] = [
    ("Dimensions", PcIcons.infoDimensions, 50),
    ("Color profile", PcIcons.infoProfile, 35),
    ("Time-lapse settings", PcIcons.infoVideo, 60),
    ("Canvas properties", PcIcons.tabCanvas, 40),
]

/// Custom Canvas, full screen: a panel of sections (dimensions, colour profile, time-lapse settings,
/// canvas properties) on the left, the chosen one on the right with the canvas's name, Cancel and
/// Done; sizes are typed on a keypad of its own, in pixels, inches, centimetres or millimetres. It
/// edits the gallery's chosen preset, or makes a new canvas (`onMade` then).
struct CustomCanvas: View {
    let model: GalleryModel
    let onMade: () -> Void

    @State private var section = 0
    @State private var name: String
    /// The size in the unit chosen.
    @State private var width: Float
    @State private var height: Float
    @State private var dpi: Float
    @State private var unit: SizeUnit
    @State private var field: Field? = .width
    /// What has been typed into the field, until it is left.
    @State private var typed = ""
    @State private var background: UInt32
    /// The background's colour while its picker is open.
    @State private var picking: HeldColor?
    @State private var hidden: Bool
    @State private var resolution: UInt32
    @State private var quality: UInt32
    @State private var hevc: Bool
    @State private var profile: String
    /// Which profiles the list shows.
    @State private var cmyk: Bool
    @State private var profileAsked = false

    init(model: GalleryModel, onMade: @escaping () -> Void) {
        self.model = model
        self.onMade = onMade
        let start = model.editingPreset.map { model.presets[$0] } ?? CanvasPreset(
            name: "Untitled Canvas", unit: "px", sizeWidth: 1080, sizeHeight: 1080, dpi: 300, background: 0xFF_FFFF,
            backgroundHidden: false, timeLapseResolution: 0, timeLapseQuality: 1, timeLapseHevc: false,
            profile: "sRGB IEC61966-2.1"
        )
        _name = State(initialValue: start.name)
        _width = State(initialValue: start.sizeWidth)
        _height = State(initialValue: start.sizeHeight)
        _dpi = State(initialValue: start.dpi)
        _unit = State(initialValue: SizeUnit.allCases.first { $0.key == start.unit } ?? .pixels)
        _background = State(initialValue: start.background)
        _hidden = State(initialValue: start.backgroundHidden)
        _resolution = State(initialValue: start.timeLapseResolution)
        _quality = State(initialValue: start.timeLapseQuality)
        _hevc = State(initialValue: start.timeLapseHevc)
        _profile = State(initialValue: start.profile)
        _cmyk = State(initialValue: isCmykProfile(name: start.profile))
    }

    private func pickBackground() {
        picking = HeldColor(rgb: background) { background = $0 }
    }

    private func preset() -> CanvasPreset {
        let shown = name.trimmingCharacters(in: .whitespaces)
        return CanvasPreset(
            name: shown.isEmpty ? "Untitled Canvas" : shown, unit: unit.key, sizeWidth: width, sizeHeight: height,
            dpi: dpi, background: background, backgroundHidden: hidden, timeLapseResolution: resolution,
            timeLapseQuality: quality, timeLapseHevc: hevc, profile: profile
        )
    }

    /// What typing has made of the field so far goes into it.
    private func commit() {
        if let v = Float(typed), v > 0 {
            switch field {
            case .width: width = v
            case .height: height = v
            case .dpi: dpi = min(max(v, 1), 9999)
            case nil: break
            }
        }
        typed = ""
    }

    /// The size kept, in another unit (straight from one length to another; through the pixels
    /// otherwise).
    private func convert(to next: SizeUnit) {
        let p = preset()
        let from = perInch(unit: unit.key)
        func change(_ v: Float, _ px: UInt32) -> Float {
            guard let to = perInch(unit: next.key) else { return Float(px) }
            if let from {
                return v / from * to
            }
            return Float(px) / dpi * to
        }
        width = change(width, presetWidth(preset: p))
        height = change(height, presetHeight(preset: p))
        unit = next
    }

    private func create() {
        commit()
        if model.finishCustom(preset()) {
            onMade()
        }
    }

    private func press(_ k: String) {
        switch k {
        case "⌫":
            if !typed.isEmpty {
                typed.removeLast()
            }
        case ".":
            if !typed.contains("."), field != .dpi, unit != .pixels {
                typed += "."
            }
        default:
            if typed.count < 7 {
                typed += k
            }
        }
    }

    private func group<Content: View>(@ViewBuilder _ content: () -> Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: 16, style: .circular)
        return VStack(spacing: 0) {
            content()
        }
        .frame(maxWidth: .infinity)
        .background(shape.fill(panelFill))
        .clipShape(shape)
    }

    private var line: some View {
        Rectangle().fill(screenFill).frame(height: 1)
    }

    private func sizeRow(_ label: String, _ value: String, unit: String, editing isEditing: Bool, tap: @escaping () -> Void) -> some View {
        HStack(spacing: 0) {
            Text(verbatim: label).foregroundStyle(ink)
            Spacer(minLength: 0)
            Text(verbatim: value).foregroundStyle(isEditing ? editing : ink)
            if !unit.isEmpty {
                Text(verbatim: " \(unit)").foregroundStyle(ink)
            }
        }
        .font(.system(size: 15))
        .padding(.horizontal, 16)
        .frame(height: 47)
        .contentShape(Rectangle())
        .onTapGesture(perform: tap)
    }

    private func choiceRow(_ label: String, chosen: Bool, detail: String? = nil, tick: Bool = false, tap: @escaping () -> Void) -> some View {
        HStack(spacing: 0) {
            Text(verbatim: label)
            Spacer(minLength: 0)
            if let detail {
                Text(verbatim: detail)
            }
            if tick, chosen {
                PcIconView(PcIcons.menuTick, size: 20, color: .white)
            }
        }
        .font(.system(size: 15))
        .foregroundStyle(chosen ? Color.white : ink)
        .padding(.horizontal, 16)
        .frame(height: 47)
        .background(chosen ? Pc.accent : Color.clear)
        .contentShape(Rectangle())
        .onTapGesture(perform: tap)
    }

    private func switchLine(_ label: String, on: Bool, change: @escaping () -> Void) -> some View {
        HStack(spacing: 0) {
            Text(verbatim: label)
                .font(.system(size: 15))
                .foregroundStyle(ink)
            Spacer(minLength: 0)
            PcSwitch(on: on)
        }
        .padding(.leading, 16)
        .padding(.trailing, 8)
        .frame(height: 47)
        .contentShape(Rectangle())
        .onTapGesture(perform: change)
    }

    /// RGB or CMYK: which profiles the page lists, the chosen half blue.
    private var kindSwitch: some View {
        HStack(spacing: 0) {
            ForEach(0 ..< 2, id: \.self) { i in
                let on = cmyk == (i == 1)
                Text(verbatim: i == 0 ? "RGB" : "CMYK")
                    .font(.system(size: 15))
                    .foregroundStyle(on ? Color.white : switchIdle)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(Capsule().fill(on ? Pc.accent : Color.clear))
                    .contentShape(Capsule())
                    .onTapGesture { cmyk = i == 1 }
            }
        }
        .padding(2)
        .frame(height: 34)
        .background(Capsule().fill(switchTrack))
    }

    private var rail: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(verbatim: "Custom Canvas")
                .font(.system(size: 22, weight: .bold))
                .foregroundStyle(ink)
                .padding(.leading, 24)
                .padding(.top, 16)
                .padding(.bottom, 20)
            ForEach(sections.indices, id: \.self) { i in
                let chosen = i == section
                let tint = chosen ? Color.white : idleInk
                // icons centred 43 in, whatever their size; the names 69 in
                HStack(spacing: 7) {
                    PcIconView(sections[i].icon, size: sections[i].size, color: tint)
                        .frame(width: 38)
                    Text(verbatim: sections[i].label)
                        .font(.system(size: 15))
                        .foregroundStyle(tint)
                    Spacer(minLength: 0)
                }
                .padding(.leading, 16)
                .frame(width: 304, height: 52)
                .background(Capsule().fill(chosen ? Pc.accent : Color.clear))
                .contentShape(Capsule())
                .onTapGesture {
                    commit()
                    section = i
                }
                .padding(.leading, 8)
                .padding(.bottom, 10)
            }
            Spacer(minLength: 0)
        }
        .frame(width: 319)
        .frame(maxHeight: .infinity)
        .background(RoundedRectangle(cornerRadius: 20, style: .circular).fill(panelFill))
        .padding(.leading, 10)
        .padding(.vertical, 10)
    }

    @ViewBuilder private var page: some View {
        switch section {
        case 0:
            let p = preset()
            VStack(spacing: 24) {
                group {
                    sizeRow("Width", field == .width && !typed.isEmpty ? typed : presetNumber(v: width), unit: unit.mark, editing: field == .width) {
                        commit()
                        field = .width
                    }
                    line
                    sizeRow("Height", field == .height && !typed.isEmpty ? typed : presetNumber(v: height), unit: unit.mark, editing: field == .height) {
                        commit()
                        field = .height
                    }
                }
                group {
                    sizeRow("DPI", field == .dpi && !typed.isEmpty ? typed : "\(wholeNumber(dpi))", unit: "", editing: field == .dpi) {
                        commit()
                        field = .dpi
                    }
                    line
                    sizeRow("Maximum Layers", "\(layersFor(width: presetWidth(preset: p), height: presetHeight(preset: p)))", unit: "", editing: false) {}
                }
            }
        case 1:
            let names = cmyk ? cmykProfiles() : colorProfiles()
            VStack(spacing: 24) {
                kindSwitch
                group {
                    ForEach(names.indices, id: \.self) { i in
                        if i > 0 {
                            line
                        }
                        choiceRow(names[i], chosen: names[i] == profile) { profile = names[i] }
                    }
                }
            }
        case 2:
            VStack(spacing: 24) {
                group {
                    let sizes = [("1080p", "1920 × 1080px"), ("2K", "2732 × 2048px"), ("4K", "3840 × 2160px")]
                    ForEach(sizes.indices, id: \.self) { i in
                        if i > 0 {
                            line
                        }
                        choiceRow(sizes[i].0, chosen: UInt32(i) == resolution, detail: sizes[i].1) { resolution = UInt32(i) }
                    }
                }
                group {
                    let levels = ["Low quality", "Good quality", "Studio quality", "Lossless"]
                    ForEach(levels.indices, id: \.self) { i in
                        if i > 0 {
                            line
                        }
                        choiceRow(levels[i], chosen: UInt32(i) == quality, tick: true) { quality = UInt32(i) }
                    }
                }
                group {
                    switchLine("HEVC", on: hevc) { hevc.toggle() }
                }
            }
        default:
            VStack(spacing: 24) {
                group {
                    HStack(spacing: 0) {
                        Text(verbatim: "Background color")
                            .font(.system(size: 15))
                            .foregroundStyle(ink)
                        Spacer(minLength: 0)
                        Circle()
                            .fill(Color(argb: 0xFF00_0000 | background))
                            .overlay(Circle().strokeBorder(Color(argb: 0xFF55_5555), lineWidth: 1))
                            .frame(width: 24, height: 24)
                    }
                    .padding(.horizontal, 16)
                    .frame(height: 47)
                    .contentShape(Rectangle())
                    .onTapGesture(perform: pickBackground)
                }
                group {
                    switchLine("Background hidden", on: hidden) { hidden.toggle() }
                }
            }
        }
    }

    private func capsuleWord(_ label: String, tap: @escaping () -> Void) -> some View {
        Text(verbatim: label)
            .font(.system(size: 17))
            .foregroundStyle(capsuleInk)
            .padding(.horizontal, 13)
            .frame(maxHeight: .infinity)
            .contentShape(Rectangle())
            .onTapGesture(perform: tap)
    }

    private func topRow(wide: Bool, screenWidth: CGFloat) -> some View {
        ZStack(alignment: .topLeading) {
            TextField("", text: $name)
                .font(.system(size: 22, weight: .bold))
                .foregroundStyle(ink)
                .tint(Pc.accent)
                .multilineTextAlignment(wide ? .center : .leading)
                .onChange(of: name) { _, value in
                    if value.count > 64 {
                        name = String(value.prefix(64))
                    }
                }
                .frame(width: wide ? screenWidth - 372 - 32 : screenWidth - 394 - 160)
                .offset(x: wide ? 372 : 394, y: 28)
            HStack(spacing: 13) {
                Spacer(minLength: 0)
                // Cancel, and Import beside it on the colour profile page
                HStack(spacing: 0) {
                    capsuleWord("Cancel") { model.closeCustom() }
                    if section == 1 {
                        capsuleWord("Import") { profileAsked = true }
                    }
                }
                .frame(height: 43)
                .background(Capsule().fill(capsuleFill))
                .overlay(Capsule().strokeBorder(capsuleEdge, lineWidth: 1))
                PcIconView(PcIcons.doneTick, size: 40, color: .white)
                    .frame(width: 44, height: 45)
                    .background(Capsule().fill(doneBlue))
                    .contentShape(Capsule())
                    .onTapGesture(perform: create)
            }
            .padding(.top, 20)
            .padding(.trailing, 20)
        }
    }

    var body: some View {
        GeometryReader { screen in
            let wide = screen.size.width > screen.size.height
            ZStack(alignment: .topLeading) {
                screenFill
                    .contentShape(Rectangle())
                    .onTapGesture {}
                rail
                topRow(wide: wide, screenWidth: screen.size.width)
                page
                    .padding(.leading, 372)
                    .padding(.trailing, 32)
                    .padding(.top, 120)
                if section == 0, field != nil {
                    Keypad(
                        wide: wide, room: screen.size.height - typedBottom, unit: unit,
                        onUnit: {
                            commit()
                            convert(to: $0)
                        },
                        onKey: press,
                        onNext: {
                            commit()
                            switch field {
                            case .width: field = .height
                            case .height: field = .dpi
                            default: field = .width
                            }
                        },
                        onDone: {
                            commit()
                            field = nil
                        }
                    )
                    .frame(width: screen.size.width, height: screen.size.height, alignment: .bottom)
                }
                // the background's colour, in a compact picker hanging from its swatch (60 in from
                // the right, under the row) at the screen's edge
                if let picking, section == 3 {
                    Color.clear
                        .contentShape(Rectangle())
                        .onTapGesture { self.picking = nil }
                    CompactColors(c: picking, arrowX: 200, screen: screen.size)
                        .offset(x: screen.size.width - compactColorsWidth, y: 168)
                }
            }
        }
        .ignoresSafeArea()
        #if DEBUG
        .onAppear {
            if ScreenCapture.galleryShows == "gallery-custom-colour" {
                section = 3
                pickBackground()
            }
        }
        #endif
        .fileImporter(isPresented: $profileAsked, allowedContentTypes: [.item]) { result in
            if case let .success(url) = result, let bytes = url.readPicked(), let imported = importProfile(bytes: bytes) {
                profile = imported
                cmyk = isCmykProfile(name: imported)
            }
        }
    }
}

/// The keypad: units on the left, the digits, and Backspace, Next and Done on the right. Upright its
/// keys are 120 x 71 (8 apart down) in a 342 panel; on a `wide` screen 150 x 87.5 (9 apart) in a 457
/// one, the rows from 47 below its top. Where the screen leaves it less than that (`room`), it and
/// its keys are that much shorter (to half), so it stays under the rows it types into.
private struct Keypad: View {
    let wide: Bool
    let room: CGFloat
    let unit: SizeUnit
    let onUnit: (SizeUnit) -> Void
    let onKey: (String) -> Void
    let onNext: () -> Void
    let onDone: () -> Void

    private func key(_ label: String, fill: Color, ink: Color, size: CGFloat, width: CGFloat, height: CGFloat, tap: @escaping () -> Void) -> some View {
        Text(verbatim: label)
            .font(.system(size: size))
            .foregroundStyle(ink)
            .frame(width: width, height: height)
            .background(RoundedRectangle(cornerRadius: 8, style: .circular).fill(fill))
            .contentShape(Rectangle())
            .onTapGesture(perform: tap)
    }

    var body: some View {
        let full: CGFloat = wide ? 457 : 342
        let fit = min(max(room / full, 0.5), 1)
        let w: CGFloat = wide ? 150 : 120
        let h = (wide ? 87.5 : 71) * fit
        let down = (wide ? 9 : 8) * fit
        let across: CGFloat = 9
        let chosenInk = Look.shared.light ? Color.white : Color(argb: 0xFF1F_1F1F)
        HStack(alignment: .top, spacing: 30) {
            VStack(spacing: down) {
                ForEach(SizeUnit.allCases, id: \.self) { u in
                    key(u.label, fill: u == unit ? unitChosen : keyFill, ink: u == unit ? chosenInk : ink, size: 15, width: w, height: h) { onUnit(u) }
                }
            }
            VStack(spacing: down) {
                ForEach([["7", "8", "9"], ["4", "5", "6"], ["1", "2", "3"]], id: \.self) { row in
                    HStack(spacing: across) {
                        ForEach(row, id: \.self) { k in
                            key(k, fill: digitFill, ink: ink, size: 26, width: w, height: h) { onKey(k) }
                        }
                    }
                }
                HStack(spacing: across) {
                    key("0", fill: digitFill, ink: ink, size: 26, width: w * 2 + across, height: h) { onKey("0") }
                    key(".", fill: digitFill, ink: ink, size: 26, width: w, height: h) { onKey(".") }
                }
            }
            VStack(spacing: down) {
                key("Backspace", fill: keyFill, ink: ink, size: 15, width: w, height: h) { onKey("⌫") }
                key("Next", fill: keyFill, ink: ink, size: 15, width: w, height: h, tap: onNext)
                key("Done", fill: keyFill, ink: ink, size: 15, width: w, height: h * 2 + down, tap: onDone)
            }
        }
        .padding(.top, wide ? 47 * fit : 0)
        .frame(maxWidth: .infinity, alignment: .center)
        .frame(height: full * fit, alignment: wide ? .top : .center)
        .background(keypadFill)
    }
}
