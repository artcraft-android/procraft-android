import PhotosUI
import SwiftUI
import UIKit
import UniformTypeIdentifiers

private func themed(_ dark: UInt32, _ light: UInt32) -> Color {
    Color(argb: Look.shared.light ? light : dark)
}

private var screenFill: Color { themed(0xFF1F_1F1F, 0xFFF3_F3F3) }
private var railFill: Color { themed(0xFF27_2727, 0xFFEB_EBEB) }
private var cardFill: Color { themed(0xFF25_2525, 0xFFEB_EBEB) }
private var ink: Color { themed(0xFFC1_C1C1, 0xFF3B_3B3B) }
private var railIdle: Color { themed(0xFFAA_AAAA, 0xFF3D_3D3D) }
private var photoFill: Color { themed(0xFF30_3030, 0xFFB3_B3B3) }
private var photoGlyph: Color { themed(0xFF4E_4E4E, 0xFFF5_F5F5) }
private var placeholder: Color { themed(0xFF60_5F63, 0xFFB8_B8BC) }
private var signLine: Color { themed(0xFF49_4949, 0xFF9A_9A9A) }
private var capsuleFill: Color { themed(0xFF1A_1A1A, 0xFFFB_FBFB) }
private var capsuleEdge: Color { themed(0xFF37_3737, 0xFFE0_E0E0) }
private var cardEdge: Color { themed(0xFF37_3737, 0xFFDA_DADA) }
private var pageInk: Color { themed(0xFF79_7979, 0xFF3E_3E3E) }
// "Sign here" and its clear button: the photo's glyph in dark, darker greys on light
private var signInk: Color { Look.shared.light ? Color(argb: 0xFF66_6666) : photoGlyph }
private var signClear: Color { Look.shared.light ? Color(argb: 0xFF39_3939) : photoGlyph }
private let doneBlue = Color(argb: 0xFF01_90FF)
private let cardCorner: CGFloat = 25
/// Where a wide screen's page starts and ends, beside the pages' card.
private let wideStart: CGFloat = 358
private let wideEnd: CGFloat = 28

private let tabs: [(label: String, icon: PcIcon, size: CGFloat)] = [
    ("About this artwork", PcIcons.infoAbout, 39),
    ("Dimensions", PcIcons.infoDimensions, 51),
    ("Layers", PcIcons.infoLayers, 35),
    ("Color profile", PcIcons.infoProfile, 33),
    ("Video settings", PcIcons.infoVideo, 55),
    ("Statistics", PcIcons.infoStatistics, 35),
]

private func dateText(_ date: Date) -> String {
    let day = DateFormatter()
    day.dateStyle = .long
    let time = DateFormatter()
    time.timeStyle = .short
    return "\(day.string(from: date)) at \(time.string(from: date))"
}

private func megabytes(_ bytes: UInt64) -> String {
    String(format: "%.2f MB", Double(bytes) / 1_000_000)
}

/// The licences of what the app is made with, over everything, closed by its Done.
struct LicencesPage: View {
    let onDone: () -> Void

    /// The notices, a paragraph each so a long list reads as it scrolls.
    @State private var paragraphs: [String] = []

    private static func read() -> [String] {
        func bundled(_ name: String, _ kind: String, in folder: String? = nil) -> String {
            guard let url = Bundle.main.url(forResource: name, withExtension: kind, subdirectory: folder) else { return "" }
            return (try? String(contentsOf: url, encoding: .utf8)) ?? ""
        }
        let text = "The interface font is Inter, by The Inter Project Authors.\n\n"
            + bundled("Inter-OFL", "txt", in: "fonts")
            + "\n\nThe app keeps Lucide icons (lucide.dev), by Lucide Icons and Contributors.\n\n"
            + bundled("licenses", "txt")
        return text.components(separatedBy: "\n\n")
    }

    var body: some View {
        ZStack(alignment: .top) {
            screenFill
                .contentShape(Rectangle())
                .onTapGesture {}
            ScrollView(.vertical, showsIndicators: true) {
                LazyVStack(alignment: .leading, spacing: 12) {
                    ForEach(paragraphs.indices, id: \.self) { i in
                        Text(verbatim: paragraphs[i])
                            .font(.system(size: 13))
                            .foregroundStyle(ink)
                    }
                }
                .padding(.horizontal, 28)
                .padding(.top, 100)
                .padding(.bottom, 40)
            }
            ToolHeader(title: "Licences", buttons: [], onDone: onDone)
        }
        .task {
            let read = await Task.detached(priority: .userInitiated) { Self.read() }.value
            paragraphs = read
        }
    }
}

/// Rows of a name and its value in a rounded card, a line between.
private struct Rows: View {
    let rows: [(String, String)]
    var tall: CGFloat = 50

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: cardCorner, style: .circular)
        VStack(spacing: 0) {
            ForEach(rows.indices, id: \.self) { i in
                if i > 0 {
                    Rectangle().fill(screenFill).frame(height: 1)
                }
                HStack(spacing: 0) {
                    Text(verbatim: rows[i].0)
                    Spacer(minLength: 8)
                    Text(verbatim: rows[i].1)
                }
                .font(.system(size: 17))
                .foregroundStyle(ink)
                .lineLimit(1)
                .padding(.horizontal, 16)
                .frame(height: tall)
            }
        }
        .frame(maxWidth: .infinity)
        .background(shape.fill(cardFill))
        .clipShape(shape)
    }
}

/// Canvas information, full screen: a rail of pages (about the artwork, its dimensions, layers,
/// colour profile, video and statistics), the artwork's name to edit as the title, Cancel and Done.
struct CanvasInfoPage: View {
    let model: CanvasModel
    let screen: CGSize

    @State private var page = 0
    @State private var name: String
    @State private var author: String
    @State private var photo: UIImage?
    /// A photo picked, kept on Done.
    @State private var newPhoto: Data?
    @State private var photoItem: PhotosPickerItem?
    @State private var photoAsked = false
    /// The profile picked on its page, set on Done.
    @State private var profile: String
    @State private var askProfile = false
    @State private var profileAsked = false
    /// Strokes of points across the signature's box, 0..1.
    @State private var signature: [[CGPoint]]
    private let info: ArtworkInfo

    init(model: CanvasModel, screen: CGSize) {
        self.model = model
        self.screen = screen
        let session = model.session
        let about = session.artworkInfo()
        info = about
        _name = State(initialValue: session.name())
        _author = State(initialValue: about.author)
        _profile = State(initialValue: session.profile())
        let folder = model.dir as NSString
        _photo = State(initialValue: UIImage(contentsOfFile: folder.appendingPathComponent("about/photo.jpg")))
        let strokes = readSignature(dir: model.dir).map { flat in
            stride(from: 0, to: flat.count - 1, by: 2).map { CGPoint(x: CGFloat(flat[$0]), y: CGFloat(flat[$0 + 1])) }
        }
        _signature = State(initialValue: strokes)
    }

    private var wide: Bool {
        screen.width > screen.height
    }

    private func close() {
        model.canvasInfoOpen = false
    }

    private func done(asked: Bool = false) {
        let session = model.session
        // another profile is asked about first, as measured (a time-lapse recorded or not)
        if !asked, profile != session.profile() {
            askProfile = true
            return
        }
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        if !trimmed.isEmpty, trimmed != session.name() {
            _ = session.setName(name: trimmed)
        }
        session.setAuthor(name: author)
        keepAbout()
        if profile != session.profile() {
            changeProfile()
        }
        close()
    }

    /// Keeps the author's photo and the signature in the artwork's folder, off the main thread.
    private func keepAbout() {
        let dir = model.dir
        let photo = newPhoto
        let strokes = signature.map { stroke in stroke.flatMap { [Float($0.x), Float($0.y)] } }
        DispatchQueue.global(qos: .utility).async { [model] in
            var failed: String?
            if let photo, (try? writePhoto(dir: dir, bytes: photo)) == nil {
                failed = "The photo couldn't be kept."
            } else if (try? writeSignature(dir: dir, strokes: strokes)) == nil {
                failed = "The signature couldn't be kept."
            }
            if let failed {
                DispatchQueue.main.async { model.showNotice(failed, warning: true) }
            }
        }
    }

    /// Puts the canvas in the profile picked, its colours and the current colour turned into it.
    private func changeProfile() {
        let session = model.session
        let from = session.profile()
        let rgb = model.colorRgb()
        guard session.changeProfile(name: profile) else { return }
        model.refresh()
        model.setRgb(convertColor(rgb: rgb, from: from, to: profile))
    }

    private var pagesCard: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(verbatim: "Canvas Info")
                .font(.system(size: 24, weight: .bold))
                .foregroundStyle(ink)
                .lineLimit(1)
                .padding(.leading, 24)
                .padding(.top, 22)
            VStack(spacing: 3) {
                ForEach(tabs.indices, id: \.self) { i in
                    let chosen = i == page
                    HStack(spacing: 0) {
                        PcIconView(tabs[i].icon, size: tabs[i].size, color: chosen ? .white : railIdle)
                            .frame(width: 60)
                        Text(verbatim: tabs[i].label)
                            .font(.system(size: 17))
                            .foregroundStyle(chosen ? Color.white : pageInk)
                            .lineLimit(1)
                        Spacer(minLength: 0)
                    }
                    .frame(height: 53)
                    .background(Capsule().fill(chosen ? Pc.accent : Color.clear))
                    .contentShape(Capsule())
                    .onTapGesture { page = i }
                }
            }
            .padding(.horizontal, 8)
            .padding(.top, 17)
            Spacer(minLength: 0)
        }
        .frame(width: 320)
        .frame(maxHeight: .infinity)
        .background(RoundedRectangle(cornerRadius: 20, style: .circular).fill(railFill))
        .overlay(RoundedRectangle(cornerRadius: 20, style: .circular).strokeBorder(cardEdge, lineWidth: 1))
        .padding(.leading, 10)
        .padding(.vertical, 10)
    }

    /// The rail of pages.
    private var rail: some View {
        VStack(spacing: 3) {
            ForEach(tabs.indices, id: \.self) { i in
                let chosen = i == page
                PcIconView(tabs[i].icon, size: tabs[i].size, color: chosen ? .white : railIdle)
                    .frame(width: 65, height: 53)
                    .background(Capsule().fill(chosen ? Pc.accent : Color.clear))
                    .contentShape(Capsule())
                    .onTapGesture { page = i }
            }
            Spacer(minLength: 0)
        }
        .padding(.leading, 8)
        .padding(.trailing, 8)
        .padding(.top, 46)
        .frame(width: 81)
        .frame(maxHeight: .infinity)
        .background(railFill)
    }

    private func word(_ label: String, tap: @escaping () -> Void) -> some View {
        Text(verbatim: label)
            .font(.system(size: 17))
            .foregroundStyle(Color(argb: 0xFFA5_A5A5))
            .padding(.horizontal, 13)
            .frame(maxHeight: .infinity)
            .contentShape(Rectangle())
            .onTapGesture(perform: tap)
    }

    private var topRow: some View {
        ZStack(alignment: .topLeading) {
            TextField("", text: $name)
                .font(.system(size: 18, weight: .bold))
                .foregroundStyle(ink)
                .tint(.white)
                .multilineTextAlignment(wide ? .center : .leading)
                .frame(width: wide ? screen.width - wideStart - wideEnd : 400)
                .offset(x: wide ? wideStart : 101, y: 30)
            HStack(spacing: 13) {
                Spacer(minLength: 0)
                // Cancel, with Import before it on the colour profile page
                HStack(spacing: 0) {
                    if page == 3 {
                        word("Import") { profileAsked = true }
                    }
                    word("Cancel", tap: close)
                }
                .frame(height: 43)
                .background(Capsule().fill(capsuleFill))
                .overlay(Capsule().strokeBorder(capsuleEdge, lineWidth: 1))
                PcIconView(PcIcons.doneTick, size: 40, color: .white)
                    .frame(width: 44, height: 45)
                    .background(Capsule().fill(doneBlue))
                    .contentShape(Capsule())
                    .onTapGesture { done() }
            }
            .padding(.top, 20)
            .padding(.trailing, 20)
        }
    }

    /// The profiles to pick from in a rounded card, a line between; the one picked to change to blue,
    /// the current one not marked.
    private func profileRows(_ names: [String], picked: String?) -> some View {
        let shape = RoundedRectangle(cornerRadius: cardCorner, style: .circular)
        return VStack(spacing: 0) {
            ForEach(names.indices, id: \.self) { i in
                if i > 0 {
                    Rectangle().fill(screenFill).frame(height: 2)
                }
                Text(verbatim: names[i])
                    .font(.system(size: 14))
                    .foregroundStyle(names[i] == picked ? Color.white : ink)
                    .lineLimit(1)
                    .padding(.horizontal, 16)
                    .frame(maxWidth: .infinity, minHeight: 47, maxHeight: 47, alignment: .leading)
                    .background(names[i] == picked ? Pc.accent : Color.clear)
                    .contentShape(Rectangle())
                    .onTapGesture { profile = names[i] }
            }
        }
        .background(shape.fill(cardFill))
        .clipShape(shape)
    }

    private var created: Date? {
        info.createdMs > 0 ? Date(timeIntervalSince1970: Double(info.createdMs) / 1000) : savedAt
    }

    private var savedAt: Date? {
        let file = (model.dir as NSString).appendingPathComponent("artwork.json")
        return (try? FileManager.default.attributesOfItem(atPath: file))?[.modificationDate] as? Date
    }

    private var folderBytes: UInt64 {
        guard let walk = FileManager.default.enumerator(at: URL(fileURLWithPath: model.dir), includingPropertiesForKeys: [.fileSizeKey]) else { return 0 }
        var total: UInt64 = 0
        for case let file as URL in walk {
            total += UInt64((try? file.resourceValues(forKeys: [.fileSizeKey]))?.fileSize ?? 0)
        }
        return total
    }

    @ViewBuilder private var pageShown: some View {
        let session = model.session
        switch page {
        case 0:
            VStack(spacing: 21) {
                AboutCard(photo: photo, author: $author, signature: $signature) { photoAsked = true }
                Rows(rows: [
                    ("Date created", created.map(dateText) ?? "—"),
                    ("Date modified", (savedAt ?? created).map(dateText) ?? "—"),
                ])
            }
        case 1:
            let c = session.info()
            let dpi = max(c.dpi, 1)
            let mm = { (px: UInt32) in "\(wholeNumber(Float(px) / dpi * 25.4))mm" }
            VStack(spacing: 21) {
                Rows(rows: [("Pixel width", "\(c.width)px"), ("Pixel height", "\(c.height)px")])
                Rows(rows: [("Physical width", mm(c.width)), ("Physical height", mm(c.height))])
                Rows(rows: [("DPI", dpi.truncatingRemainder(dividingBy: 1) == 0 ? "\(Int(dpi))" : String(format: "%.1f", dpi))])
            }
        case 2:
            let l = session.layerStats()
            VStack(spacing: 21) {
                Rows(rows: [("Maximum layers", "\(l.maximum)")])
                Rows(rows: [("Layers used", "\(l.used)"), ("Layers available", "\(l.maximum > l.used ? l.maximum - l.used : 0)")])
                Rows(rows: [
                    ("Assisted layers", "\(l.assisted)"), ("Clipping layers", "\(l.clipping)"),
                    ("Layer masks", "\(l.masks)"), ("Layer groups", "\(l.groups)"),
                ])
            }
        case 3:
            let current = session.profile()
            VStack(alignment: .leading, spacing: 0) {
                Rows(rows: [("Current color profile", current)], tall: 44)
                Text(verbatim: "Available color profiles")
                    .font(.system(size: 17))
                    .foregroundStyle(ink)
                    .padding(.leading, 16)
                    .padding(.top, 32)
                    .padding(.bottom, 8)
                // the profiles of the canvas's kind: RGB, or CMYK
                profileRows(session.isCmyk() ? cmykProfiles() : colorProfiles(), picked: profile == current ? nil : profile)
            }
        case 4:
            let lapse = model.timeLapse.lapse
            let seconds = lapse.count / Int(timeLapseFps())
            // the settings the artwork was made with: quality, resolution, codec
            let names = timeLapseSettingNames(s: lapse.settings)
            Rows(rows: [
                ("Video length", String(format: "%02d:%02d:%02d", seconds / 3600, seconds / 60 % 60, seconds % 60)),
                ("Video quality", names.first ?? "—"),
                ("Video resolution", names.dropFirst().first ?? "—"),
                ("Video file size", megabytes(lapse.bytes)),
                ("Video codec", names.last ?? "—"),
            ])
        default:
            let minutes = info.trackedMs / 60000
            Rows(rows: [
                ("Total strokes made", "\(info.strokes)"),
                ("Tracked time", "\(minutes / 60)h \(minutes % 60)m"),
                ("Total file size", megabytes(folderBytes)),
            ])
        }
    }

    var body: some View {
        ZStack(alignment: .topLeading) {
            screenFill
                .contentShape(Rectangle())
                .onTapGesture {}
            if wide {
                pagesCard
            } else {
                rail
            }
            topRow
            ScrollView(.vertical, showsIndicators: false) {
                pageShown
                    .padding(.bottom, 40)
            }
            .padding(.leading, wide ? wideStart : 105)
            .padding(.trailing, wide ? wideEnd : 24)
            .padding(.top, 100)
            AlertView(
                content: askProfile ? AlertContent(
                    title: "Change color profile?",
                    message: "Its colours keep their values and look as the new profile shows them, in the time-lapse recording too.",
                    action: "Change", destructive: false, cancel: "Don't change"
                ) : nil,
                onCancel: {
                    askProfile = false
                    profile = model.session.profile()
                    done(asked: true)
                },
                onAction: {
                    askProfile = false
                    done(asked: true)
                }
            )
        }
        .photosPicker(isPresented: $photoAsked, selection: $photoItem, matching: .images)
        .onChange(of: photoItem) { _, item in
            guard let item else { return }
            photoItem = nil
            Task { @MainActor in
                // only a picture that reads takes the old photo's place
                if let data = try? await item.loadTransferable(type: Data.self), let picture = UIImage(data: data) {
                    newPhoto = data
                    photo = picture
                }
            }
        }
        .fileImporter(isPresented: $profileAsked, allowedContentTypes: [.item]) { result in
            // a profile of the canvas's kind is picked to change to
            if case let .success(url) = result, let bytes = url.readPicked(), let imported = importProfile(bytes: bytes),
               isCmykProfile(name: imported) == model.session.isCmyk() {
                profile = imported
            }
        }
    }
}

/// The author: a photo (tap for another), "Made by" and the name, and a signature drawn on a line.
private struct AboutCard: View {
    let photo: UIImage?
    @Binding var author: String
    @Binding var signature: [[CGPoint]]
    let onPhoto: () -> Void

    /// The stroke a touch is drawing.
    @State private var drawing = false

    private func signatureBox() -> some View {
        let width: CGFloat = 401
        let height: CGFloat = 121
        return ZStack(alignment: .topLeading) {
            Canvas { context, size in
                var line = Path()
                let from = (size.width - 305) / 2
                line.move(to: CGPoint(x: from, y: 82))
                line.addLine(to: CGPoint(x: from + 305, y: 82))
                context.stroke(line, with: .color(signLine), style: StrokeStyle(lineWidth: 1.5, dash: [4, 4]))
                for stroke in signature {
                    var path = Path()
                    path.addLines(stroke.map { CGPoint(x: $0.x * size.width, y: $0.y * size.height) })
                    context.stroke(path, with: .color(ink), style: StrokeStyle(lineWidth: 2, lineCap: .round, lineJoin: .round))
                }
            }
            .contentShape(Rectangle())
            // drawn with a finger or pen over the dashed line
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { drag in
                        let p = CGPoint(x: drag.location.x / width, y: drag.location.y / height)
                        if drawing, !signature.isEmpty {
                            signature[signature.count - 1].append(p)
                        } else {
                            drawing = true
                            signature.append([p])
                        }
                    }
                    .onEnded { _ in drawing = false }
            )
            Text(verbatim: "Sign here")
                .font(.system(size: 11))
                .foregroundStyle(signInk)
                .offset(x: 50, y: 90)
                .allowsHitTesting(false)
            // always there, at the line's end
            PcIconView(PcIcons.cross, size: 14, color: cardFill)
                .frame(width: 22, height: 22)
                .background(Circle().fill(signClear))
                .contentShape(Circle())
                .onTapGesture { signature = [] }
                .offset(x: 330, y: 58)
        }
        .frame(width: width, height: height)
    }

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: cardCorner, style: .circular)
        VStack(spacing: 0) {
            ZStack {
                Circle().fill(photoFill)
                if let photo {
                    Image(uiImage: photo)
                        .resizable()
                        .scaledToFill()
                } else {
                    PcIconView(PcIcons.person, size: 48, color: photoGlyph)
                }
            }
            .frame(width: 97, height: 97)
            .clipShape(Circle())
            .contentShape(Circle())
            .onTapGesture(perform: onPhoto)
            .padding(.top, 30)
            HStack(alignment: .firstTextBaseline, spacing: 0) {
                Text(verbatim: "Made by ")
                    .foregroundStyle(ink)
                TextField("", text: $author, prompt: Text(verbatim: "Name").foregroundColor(placeholder))
                    .foregroundStyle(ink)
                    .tint(.white)
                    .frame(width: author.isEmpty ? 60 : min(CGFloat(author.count) * 12 + 16, 260))
                    .onChange(of: author) { _, value in
                        if value.count > 256 {
                            author = String(value.prefix(256))
                        }
                    }
            }
            .font(.system(size: 21))
            .padding(.top, 19)
            Spacer(minLength: 0)
            signatureBox()
        }
        .frame(maxWidth: .infinity)
        .frame(height: 300)
        .background(shape.fill(cardFill))
        .clipShape(shape)
    }
}
