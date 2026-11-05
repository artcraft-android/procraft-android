import PhotosUI
import SwiftUI
import UniformTypeIdentifiers

private func themed(_ dark: UInt32, _ light: UInt32) -> Color {
    Color(argb: Look.shared.light ? light : dark)
}

private var editorPage: Color { themed(0xFF2A_2A2A, 0xFFE5_E5E5) }
private var editorView: Color { themed(0xFF26_2626, 0xFFE5_E5E5) }
private var editorTitle: Color { themed(0xFFEE_EEEE, 0xFF33_3333) }
private var capsuleFill: Color { themed(0xFF1C_1C1C, 0xFFFF_FFFF) }
private var capsuleEdge: Color { themed(0xFF38_3838, 0xFFE0_E0E0) }
private var capsuleInk: Color { themed(0xFFAA_AAAA, 0xFF58_5858) }
private let repeatLabel = Color(argb: 0xFF80_8080)
private let libraryVeil = Color(argb: 0x8000_0000)
private var librarySheet: Color { themed(0xFF1C_1C1E, 0xFFF2_F2F7) }
private var libraryTitle: Color { themed(0xFFC1_C1C1, 0xFF3A_3A3A) }
private let libraryDim = Color(argb: 0xFF86_8686)
private var libraryEdge: Color { themed(0xFF5A_5A5A, 0xFFC8_C8C8) }
private var librarySearchFill: Color { themed(0xFF1B_1B1B, 0xFFE6_E6EA) }
private var librarySearchEdge: Color { themed(0xFF36_3636, 0xFFD8_D8DC) }
private var libraryHint: Color { themed(0xFF78_7878, 0xFF8A_8A8E) }
private let menuBlue = Color(argb: 0xFF0A_84FF)

/// The space the editor's Import button and its menu are placed in.
private let editorSpace = "sourceEditor"

/// Auto repeat's settings.
private struct Repeating: Equatable {
    var scale: Float = 0
    var rotate: Float = 0
    var overlap: Float = 1
    var hardness: Float = 0
    var mirror = false
    var pyramid = true
}

/// The brush studio's editor for one of its sources (the shape, the grain, or what the brush paints
/// metalness or roughness through): the image in a 600 square, changed by Import (a photo, a file,
/// the source library or the clipboard) or turned negative by a two-finger tap; all but the shape's
/// can also make it repeat without seams, and take the grains from the library. Cancel leaves the
/// brush as it was, the tick keeps the change.
struct SourceEditor: View {
    let studio: StudioModel
    let kind: SourceKind
    let landscape: Bool

    /// The image as edited here (empty: the round tip, or white all over), and the shipped one it
    /// is if it is one.
    @State private var image = Data()
    @State private var shipped: Int?
    /// Goes up as the image changes.
    @State private var revision = 0
    @State private var shown: UIImage?
    @State private var importing = false
    /// Where Import is, in the editor.
    @State private var importButton = CGRect.zero
    @State private var picking = false
    @State private var photo: PhotosPickerItem?
    @State private var choosingFile = false
    @State private var library = false
    @State private var repeating = false
    @State private var settings = Repeating()
    @Environment(\.displayScale) private var displayScale

    private var grain: Bool {
        kind != .shape
    }

    private var title: String {
        switch kind {
        case .shape:
            "Shape Editor"
        case .grain:
            "Grain Editor"
        case .metallic:
            "Metallic Editor"
        case .roughness:
            "Roughness Editor"
        }
    }

    /// `pixels` as Auto repeat makes them, with `how`.
    private static func repeated(_ pixels: Data, _ how: Repeating) -> Data {
        repeatableSource(
            pixels: pixels, scale: 1 + how.scale * 3, rotationDegrees: how.rotate * 360, overlap: how.overlap,
            hardness: how.hardness, mirror: how.mirror, pyramid: how.pyramid
        )
    }

    private func result() -> Data {
        repeating && !image.isEmpty ? Self.repeated(image, settings) : image
    }

    private func take(_ pixels: Data?) {
        if let pixels, !pixels.isEmpty {
            image = pixels
            shipped = nil
            revision += 1
        }
    }

    /// What decides the image shown.
    private struct Showing: Equatable {
        let revision: Int
        let repeating: Bool
        let settings: Repeating
    }

    /// The image shown, made off the main thread: repeating one takes a moment.
    private func redraw() async {
        let pixels = image
        let how = repeating && !pixels.isEmpty ? settings : nil
        let shape = !grain
        let made = await Task.detached(priority: .userInitiated) {
            SourceImages.image(how.map { Self.repeated(pixels, $0) } ?? pixels, shape: shape)
        }.value
        if !Task.isCancelled {
            shown = made
        }
    }

    private var theImage: some View {
        ZStack {
            editorView
            if let shown {
                Image(uiImage: shown)
                    .resizable()
                    .scaledToFit()
            } else if grain {
                Color.white
            } else {
                // the round tip: a white disc
                Circle().fill(Color.white)
                    .frame(width: 480, height: 480)
            }
            // a two-finger tap turns the image negative
            TwoFingerTap {
                if !image.isEmpty {
                    image = Data(image.map { 255 - $0 })
                    shipped = nil
                    revision += 1
                }
            }
        }
        .frame(width: 600, height: 601)
        .clipShape(RoundedRectangle(cornerRadius: 20, style: .circular))
    }

    private func capsule(_ label: String, width: CGFloat, on: Bool = false, action: @escaping () -> Void) -> some View {
        Text(verbatim: label)
            .font(.system(size: 17))
            .foregroundStyle(on ? Color.white : capsuleInk)
            .lineLimit(1)
            .frame(width: width, height: 43)
            .background(Capsule().fill(on ? Pc.accent : Color.clear))
            .contentShape(Capsule())
            .onTapGesture(perform: action)
    }

    private var buttons: some View {
        HStack(spacing: 13) {
            HStack(spacing: 0) {
                capsule("Import", width: 71) {
                    withAnimation(Motion.menuOpen) { importing = true }
                }
                .background(GeometryReader { g in
                    Color.clear
                        .onAppear { importButton = g.frame(in: .named(editorSpace)) }
                        .onChange(of: g.frame(in: .named(editorSpace))) { _, frame in importButton = frame }
                })
                if grain {
                    capsule("Auto repeat", width: 107, on: repeating) { repeating.toggle() }
                }
                capsule("Cancel", width: 73) { studio.sourceEditing = nil }
            }
            .frame(height: 43)
            .background(Capsule().fill(capsuleFill))
            .overlay(Capsule().strokeBorder(capsuleEdge, lineWidth: 1))
            PcIconView(PcIcons.doneTick, size: 40, color: .white)
                .frame(width: 44, height: 45)
                .background(Capsule().fill(StudioInk.done))
                .contentShape(Capsule())
                .onTapGesture {
                    studio.keepSource(kind, pixels: result(), shipped: repeating ? nil : shipped)
                }
        }
        .padding(.top, landscape ? 10 : 20)
        .padding(.trailing, landscape ? 12 : 20)
        .frame(maxWidth: .infinity, alignment: .trailing)
    }

    private var page: some View {
        ZStack(alignment: .topLeading) {
            editorPage
            // the grain repeating over the whole page
            if repeating, let shown {
                Rectangle().fill(ImagePaint(image: Image(uiImage: shown), scale: 1 / displayScale))
            } else {
                theImage
                    .offset(y: 32)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            Text(verbatim: title)
                .font(.system(size: 22, weight: .bold))
                .foregroundStyle(editorTitle)
                .offset(x: 24, y: landscape ? 16 : 26)
            buttons
            if repeating {
                RepeatPanel(settings: $settings)
                    .padding(.bottom, 30)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
            }
            if importing {
                Color.clear
                    .contentShape(Rectangle())
                    .onTapGesture { withAnimation(Motion.menuClose) { importing = false } }
                ImportMenu { choice in
                    withAnimation(Motion.menuClose) { importing = false }
                    switch choice {
                    case 0:
                        picking = true
                    case 1:
                        choosingFile = true
                    case 2:
                        library = true
                    default:
                        take(studio.pastedSource())
                    }
                }
                // under Import, its arrow pointing up at it
                .offset(x: importButton.midX - 160, y: importButton.minY + 47)
                .menuTransition(.top)
            }
        }
        .coordinateSpace(.named(editorSpace))
    }

    var body: some View {
        ZStack(alignment: .topLeading) {
            // landscape: a page over the studio from the right, its pages' card still showing
            Color.clear
                .contentShape(Rectangle())
            if landscape {
                page
                    .clipShape(UnevenRoundedRectangle(topLeadingRadius: 20, bottomLeadingRadius: 20, style: .circular))
                    .padding(.leading, 246)
            } else {
                page
            }
            if library {
                SourceLibrary(
                    grain: grain,
                    onPick: { source, index in
                        image = source.pixels
                        shipped = SourceImages.shipped(grain: grain, index: index)
                        revision += 1
                        library = false
                    },
                    onCancel: { library = false }
                )
            }
        }
        .onAppear {
            image = studio.sourcePixels(kind)
            revision += 1
            #if DEBUG
            switch ScreenCapture.sourceShows {
            case "import":
                importing = true
            case "repeat":
                repeating = true
            case "library":
                library = true
            default:
                break
            }
            #endif
        }
        .task(id: Showing(revision: revision, repeating: repeating, settings: settings)) {
            await redraw()
        }
        .photosPicker(isPresented: $picking, selection: $photo, matching: .images)
        .onChange(of: photo) { _, item in
            guard let item else { return }
            photo = nil
            Task { @MainActor in
                let data = try? await item.loadTransferable(type: Data.self)
                take(studio.source(fromPicture: data))
            }
        }
        .fileImporter(isPresented: $choosingFile, allowedContentTypes: [.image]) { result in
            if case let .success(url) = result {
                take(studio.source(fromPicture: url.readPicked()))
            }
        }
    }
}

/// A two-finger tap on what it lies over, one finger's touches left alone.
private struct TwoFingerTap: UIViewRepresentable {
    let onTap: () -> Void

    final class Coordinator: NSObject {
        var onTap: () -> Void

        init(onTap: @escaping () -> Void) {
            self.onTap = onTap
        }

        @objc func tapped() {
            onTap()
        }
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(onTap: onTap)
    }

    func makeUIView(context: Context) -> UIView {
        let view = UIView()
        view.backgroundColor = .clear
        let tap = UITapGestureRecognizer(target: context.coordinator, action: #selector(Coordinator.tapped))
        tap.numberOfTouchesRequired = 2
        view.addGestureRecognizer(tap)
        return view
    }

    func updateUIView(_ view: UIView, context: Context) {
        context.coordinator.onTap = onTap
    }
}

/// Where the image can come from, in a light popover under Import.
private struct ImportMenu: View {
    let onPick: (Int) -> Void

    var body: some View {
        Popover(width: 320, arrowX: 160, fill: Pc.lightPanel, corner: 24, side: .top) {
            VStack(alignment: .leading, spacing: 0) {
                Text(verbatim: "Image source options")
                    .font(.system(size: 22, weight: .bold))
                    .foregroundStyle(Pc.lightText)
                    .padding(.leading, 12)
                    .padding(.top, 16)
                    .padding(.bottom, 12)
                VStack(spacing: 0) {
                    SourceMenuRow(icon: PcIcons.sourcePhotoFill, label: "Import a photo", iconSize: 30) { onPick(0) }
                    Pc.lightSeparator.frame(height: 1).padding(.leading, 52)
                    SourceMenuRow(icon: PcIcons.folderFill, label: "Import a file", iconSize: 30) { onPick(1) }
                    Pc.lightSeparator.frame(height: 1).padding(.leading, 52)
                    SourceMenuRow(icon: PcIcons.sourceBooksFill, label: "Source Library", iconSize: 30) { onPick(2) }
                }
                .background(RoundedRectangle(cornerRadius: 14, style: .circular).fill(Pc.lightRow))
                Color.clear.frame(height: 20)
                SourceMenuRow(icon: PcIcons.textPaste, label: "Paste", iconSize: 40) { onPick(3) }
                    .background(RoundedRectangle(cornerRadius: 14, style: .circular).fill(Pc.lightRow))
            }
            .padding(.horizontal, 8)
            .padding(.bottom, 8)
        }
    }
}

private struct SourceMenuRow: View {
    let icon: PcIcon
    let label: String
    var iconSize: CGFloat = 24
    let action: () -> Void

    var body: some View {
        HStack(spacing: 0) {
            // in a 24 slot; icons drawn in a padded frame are shown bigger
            PcIconView(icon, size: iconSize, color: menuBlue)
                .frame(width: 24, height: 24)
            Text(verbatim: label)
                .font(.system(size: 17))
                .foregroundStyle(Pc.lightText)
                .lineLimit(1)
                .padding(.leading, 22)
            Spacer(minLength: 0)
        }
        .padding(.leading, 18)
        .frame(height: 44)
        .contentShape(Rectangle())
        .onTapGesture(perform: action)
    }
}

/// Auto repeat's settings: two columns of sliders and switches in a panel along the bottom.
private struct RepeatPanel: View {
    @Binding var settings: Repeating

    /// A number with no more than one place, none if it is whole.
    private var scaleReading: String {
        var text = String(format: "%.1f", 1 + settings.scale * 3)
        while text.hasSuffix("0") {
            text.removeLast()
        }
        return text.hasSuffix(".") ? String(text.dropLast()) : text
    }

    private var overlapReading: String {
        settings.overlap >= 1 ? "Maximal" : settings.overlap <= 0 ? "None" : "\(wholeNumber(settings.overlap * 100))%"
    }

    private var hardnessReading: String {
        settings.hardness <= 0 ? "Soft" : settings.hardness >= 1 ? "Hard" : "\(wholeNumber(settings.hardness * 100))%"
    }

    var body: some View {
        VStack(spacing: 16) {
            HStack(spacing: 16) {
                RepeatSlider(label: "Grain scale", reading: scaleReading, at: $settings.scale)
                RepeatSlider(label: "Rotate", reading: "\(wholeNumber(settings.rotate * 360))°", at: $settings.rotate)
            }
            HStack(spacing: 16) {
                RepeatSlider(label: "Border overlap", reading: overlapReading, at: $settings.overlap)
                RepeatSlider(label: "Mask hardness", reading: hardnessReading, at: $settings.hardness)
            }
            HStack(spacing: 16) {
                RepeatSwitch(label: "Mirror overlap", on: $settings.mirror)
                RepeatSwitch(label: "Pyramid blending", on: $settings.pyramid)
            }
        }
        .padding(16)
        .frame(width: 650, height: 228, alignment: .top)
        .background(RoundedRectangle(cornerRadius: 16, style: .circular).fill(editorPage))
    }
}

private struct RepeatSlider: View {
    let label: String
    let reading: String
    @Binding var at: Float

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                Text(verbatim: label)
                Spacer(minLength: 0)
                Text(verbatim: reading)
            }
            .font(.system(size: 15))
            .foregroundStyle(repeatLabel)
            .lineLimit(1)
            .padding(.horizontal, 16)
            .padding(.vertical, 6)
            // a thin track with a white knob: the position follows the finger
            GeometryReader { g in
                let w = g.size.width
                let part = CGFloat(min(max(at, 0), 1))
                ZStack(alignment: .leading) {
                    Capsule().fill(Color(argb: 0xFF3E_3E3E)).frame(height: 3)
                    Capsule().fill(Pc.accent).frame(width: w * part, height: 3)
                    Capsule().fill(Color.white)
                        .frame(width: 16, height: 10)
                        .offset(x: (w - 16) * part)
                }
                .frame(width: w, height: g.size.height)
                .contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 0).onChanged { drag in
                    at = Float(min(max(drag.location.x / max(w, 1), 0), 1))
                })
            }
            .frame(height: 20)
            .padding(.horizontal, 16)
        }
        .frame(width: 301, height: 55, alignment: .top)
    }
}

private struct RepeatSwitch: View {
    let label: String
    @Binding var on: Bool

    var body: some View {
        HStack(spacing: 0) {
            Text(verbatim: label)
                .font(.system(size: 15))
                .foregroundStyle(repeatLabel)
                .lineLimit(1)
            Spacer(minLength: 0)
            PcSwitch(on: on)
        }
        .padding(.leading, 16)
        .padding(.trailing, 8)
        .frame(width: 301, height: 40)
        .contentShape(Rectangle())
        .onTapGesture { on.toggle() }
    }
}

/// The source library: a sheet (580 x 575, 74 down) over the editor with its title and Cancel,
/// Shape Source and Grain Source with a search by name, then the shipped images three across (147
/// square, their names under them). A pick of the other kind than the one being edited isn't
/// taken.
private struct SourceLibrary: View {
    let grain: Bool
    let onPick: (LibrarySource, Int) -> Void
    let onCancel: () -> Void

    @State private var showGrains = false
    @State private var query = ""
    @State private var items: [LibrarySource] = []

    private var found: [(index: Int, source: LibrarySource)] {
        let wanted = query.trimmingCharacters(in: .whitespaces)
        return items.enumerated()
            .filter { wanted.isEmpty || $0.element.name.localizedCaseInsensitiveContains(wanted) }
            .map { (index: $0.offset, source: $0.element) }
    }

    private var tabs: some View {
        HStack(spacing: 0) {
            ForEach([false, true], id: \.self) { isGrain in
                Text(verbatim: isGrain ? "Grain Source" : "Shape Source")
                    .font(.system(size: 17))
                    .foregroundStyle(showGrains == isGrain ? Pc.menuInk : libraryDim)
                    .lineLimit(1)
                    .frame(width: 115, height: 41)
                    .contentShape(Rectangle())
                    .onTapGesture { showGrains = isGrain }
                    .padding(.trailing, 11)
            }
            Color.clear.frame(width: 8)
            search
        }
        .padding(.leading, 43)
        .padding(.top, 25)
        .frame(height: 70, alignment: .bottom)
    }

    private var search: some View {
        let shape = RoundedRectangle(cornerRadius: 22.5, style: .circular)
        return HStack(spacing: 0) {
            Canvas { context, size in
                // a magnifier: a ring and its handle
                let c = CGPoint(x: size.width * 0.44, y: size.height * 0.44)
                let r: CGFloat = 7
                context.stroke(Path(ellipseIn: CGRect(x: c.x - r, y: c.y - r, width: r * 2, height: r * 2)), with: .color(libraryHint), lineWidth: 2)
                let d = r * 0.7071
                var handle = Path()
                handle.move(to: CGPoint(x: c.x + d, y: c.y + d))
                handle.addLine(to: CGPoint(x: c.x + d + 5, y: c.y + d + 5))
                context.stroke(handle, with: .color(libraryHint), style: StrokeStyle(lineWidth: 2.4, lineCap: .round))
            }
            .frame(width: 23, height: 22)
            .padding(.leading, 11)
            ZStack(alignment: .leading) {
                if query.isEmpty {
                    Text(verbatim: "Search Library")
                        .font(.system(size: 17))
                        .foregroundStyle(libraryHint)
                        .lineLimit(1)
                }
                TextField(text: $query) { EmptyView() }
                    .font(.system(size: 17))
                    .foregroundStyle(Pc.menuInk)
                    .tint(Pc.accent)
                    .autocorrectionDisabled()
            }
            .padding(.leading, 9)
            .padding(.trailing, 12)
        }
        .frame(width: 225, height: 45)
        .background(shape.fill(librarySearchFill))
        .overlay(shape.strokeBorder(librarySearchEdge, lineWidth: 1))
    }

    private func cell(_ index: Int, _ source: LibrarySource) -> some View {
        let shape = RoundedRectangle(cornerRadius: 8, style: .circular)
        return VStack(alignment: .leading, spacing: 0) {
            ZStack {
                Color.black
                if let image = source.image {
                    Image(uiImage: image)
                        .resizable()
                        .scaledToFill()
                }
            }
            .frame(width: 147, height: 146)
            .clipShape(shape)
            .overlay(shape.strokeBorder(libraryEdge, lineWidth: 1))
            Text(verbatim: source.name)
                .font(.system(size: 13, weight: .bold))
                .foregroundStyle(libraryTitle)
                .lineLimit(1)
                .frame(height: 21, alignment: .top)
                .padding(.leading, 3)
                .padding(.top, 5)
        }
        .frame(width: 147, alignment: .leading)
        .contentShape(Rectangle())
        .onTapGesture {
            // each kind takes its own: a grain picked in the shape editor is not taken
            if showGrains == grain {
                onPick(source, index)
            }
        }
    }

    var body: some View {
        ZStack(alignment: .top) {
            // over everything, the studio's pages dimmed too
            libraryVeil
                .contentShape(Rectangle())
            VStack(alignment: .leading, spacing: 0) {
                HStack(spacing: 0) {
                    Text(verbatim: "Source Library")
                        .font(.system(size: 32, weight: .bold))
                        .foregroundStyle(libraryTitle)
                        .lineLimit(1)
                    Spacer(minLength: 0)
                    Text(verbatim: "Cancel")
                        .font(.system(size: 17))
                        .foregroundStyle(libraryDim)
                        .lineLimit(1)
                        .contentShape(Rectangle())
                        .onTapGesture(perform: onCancel)
                }
                .padding(.horizontal, 19)
                .padding(.top, 69)
                .frame(height: 108, alignment: .bottom)
                tabs
                ScrollView {
                    LazyVGrid(columns: Array(repeating: GridItem(.fixed(147), spacing: 53), count: 3), spacing: 18) {
                        ForEach(found, id: \.index) { item in
                            cell(item.index, item.source)
                        }
                    }
                    .padding(.horizontal, 16)
                    .padding(.top, 11)
                    .padding(.bottom, 24)
                }
                .scrollIndicators(.hidden)
            }
            .frame(width: 580, height: 575)
            .background(librarySheet)
            .clipShape(RoundedRectangle(cornerRadius: 26, style: .circular))
            .offset(y: 74)
        }
        .ignoresSafeArea(.keyboard)
        .onAppear { showGrains = grain }
        .task(id: showGrains) {
            let wanted = showGrains
            items = await Task.detached(priority: .userInitiated) {
                SourceImages.library(grain: wanted)
            }.value
        }
    }
}
