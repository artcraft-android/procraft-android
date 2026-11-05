import SwiftUI
import UIKit
import UniformTypeIdentifiers

private func themed(_ dark: UInt32, _ light: UInt32) -> Color {
    Color(argb: Look.shared.light ? light : dark)
}

private var sheetFill: Color { themed(0xFF53_5353, 0xFFF3_F6F7) }
private var sheetBar: Color { themed(0xFF4F_4F4F, 0xFFF3_F6F7) }
private var sheetCapsule: Color { themed(0xFF3E_3E3E, 0xFFFB_FBFB) }
private var cardFill: Color { themed(0xFF37_3737, 0xFFFA_FAFA) }
private var buttonFill: Color { themed(0xFF47_4747, 0xFFE6_E6E6) }
private var segmentTrack: Color { themed(0xFF64_6464, 0xFFE3_E3E3) }
private var capsuleInk: Color { themed(0xFFC5_C5C5, 0xFF56_5656) }
private var keyboardDisc: Color { themed(0xFF39_3939, 0xFFFB_FBFB) }
private var designTrack: Color { themed(0xFF5A_5A5A, 0xFFE3_E3E3) }
private var designTick: Color { themed(0xFF8A_8A8A, 0xFFC8_C8C8) }
private var sheetInk: Color { Look.shared.light ? .black : .white }
private let doneBlue = Color(argb: 0xFF01_90FF)
private let boxLine = Color(argb: 0xFF00_76FD)
private let boxLineArgb: UInt32 = 0xFF00_76FD
/// The box's outline and the dots on its sides (as measured, a lighter blue than the caret's).
private let boxOutline = Color(argb: 0xFF01_84FF)
/// A side's dot: how wide it is, and how near a finger takes it.
private let sideDot: CGFloat = 7.5
private let sideReach: CGFloat = 20
private let cardCorner: CGFloat = 22
/// Below this width the sheet shows its cards in two pages.
private let wideSheet: CGFloat = 900

/// The size slider runs 1 to 2000 pixels, finer at the small end.
private func sliderToSize(_ v: Float) -> Float {
    1 + 1999 * v * v
}

private func sizeToSlider(_ size: Float) -> Float {
    min(max((size - 1) / 1999, 0), 1).squareRoot()
}

/// Text is laid out and drawn by the core; here are the fonts it is given: the bundled Inter, the
/// system's, and the imported ones kept in the app's files.
enum TextFonts {
    private static let ready = DispatchGroup()
    private static var prepared = false

    /// Gives the core its fonts off the main thread, once.
    static func prepare() {
        guard !prepared else { return }
        prepared = true
        ready.enter()
        DispatchQueue.global(qos: .utility).async {
            if let folder = Bundle.main.url(forResource: "fonts", withExtension: nil),
               let files = try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil) {
                for file in files where file.pathExtension == "ttf" {
                    _ = addTextFontFile(path: file.path)
                }
            }
            // text names a generic family: Inter stands in unless the system has better
            setTextFamilyAlias(name: "sans-serif", family: "Inter")
            addSystemTextFonts()
            let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
                ?? URL(fileURLWithPath: NSTemporaryDirectory())
            let fonts = support.appendingPathComponent("fonts")
            try? FileManager.default.createDirectory(at: fonts, withIntermediateDirectories: true)
            setTextFontFolder(dir: fonts.path)
            ready.leave()
        }
    }

    /// Waits until the core has its fonts (text laid out before would show nothing).
    static func wait() {
        ready.wait()
    }
}

/// Text being edited: what the core is told, what the keyboard holds, and the style sheet.
@Observable
final class TextTool {
    @ObservationIgnored private unowned let model: CanvasModel

    /// The text as it stands; none when no text is being edited.
    private(set) var info: TextInfo?
    /// What the hidden field holds, and its selection (UTF-16 places).
    private(set) var field = ""
    private(set) var selection = NSRange(location: 0, length: 0)
    /// The style sheet is open over where the keyboard goes.
    private(set) var styleOpen = false
    /// The keyboard is wanted (a touch on the text asks for it).
    var keyboard = true
    /// The typefaces and styles on offer.
    private(set) var families: [TextTypeface] = []
    private(set) var styles: [String] = []

    /// The text as it was when the style sheet opened, for its Cancel.
    @ObservationIgnored private var beforeStyle: TextInfo?
    @ObservationIgnored private let queue = DispatchQueue(label: "text", qos: .userInitiated)
    @ObservationIgnored private let waiting = NSLock()
    @ObservationIgnored private var drawAsked = false

    init(model: CanvasModel) {
        self.model = model
    }

    private func start(_ next: TextInfo, keyboard wanted: Bool = true) {
        keyboard = wanted
        info = next
        field = next.text
        selection = NSRange(location: 0, length: (next.text as NSString).length)
        styleOpen = false
        TextFonts.wait()
        families = textTypefaces()
        styles = textStyles()
        render()
        model.refresh()
    }

    /// New text in the middle of the canvas, all of it selected for typing over: a medium sans, sized
    /// and placed as new text looks on screen (above the keyboard, about a quarter of the way down).
    func add() {
        model.closePanels()
        let c = model.colorRgb()
        let base = TextInfo(
            text: "Text", font: "sans-serif", style: "Medium", size: 0, kerning: 0, tracking: 0, leading: 0, baseline: 0,
            opacity: 1, align: .center, underline: false, outline: false, vertical: false, allCaps: false,
            r: Float((c >> 16) & 255) / 255, g: Float((c >> 8) & 255) / 255, b: Float(c & 255) / 255,
            x: 0, y: 0, width: 0
        )
        let centre = model.screenCentre
        let placed = model.session.newTextPlaced(
            base: base, width: Float(centre.x * 2), height: Float(centre.y * 2), density: Float(model.pixels)
        )
        guard model.session.textBegin(id: nil, info: placed) != nil else {
            model.noRoomForLayers()
            return
        }
        start(placed)
    }

    /// Edits a text layer's text again.
    func edit(_ l: LayerInfo) {
        guard let text = model.session.layerText(id: l.id) else { return }
        model.closePanels()
        guard model.session.textBegin(id: l.id, info: text) != nil else { return }
        start(text)
    }

    /// Text layer `id` back in its box, selected but with the keyboard away (undo and redo of it).
    func resume(_ id: UInt64) -> Bool {
        guard let text = model.session.layerText(id: id), model.session.textBegin(id: id, info: text) != nil else { return false }
        start(text, keyboard: false)
        return true
    }

    /// The keyboard changed what the field holds.
    func fieldChanged(_ text: String, selection next: NSRange) {
        field = text
        selection = next
        if text != info?.text {
            change { $0.text = text }
        }
    }

    func selectionChanged(_ next: NSRange) {
        selection = next
    }

    func change(_ apply: (inout TextInfo) -> Void) {
        guard var next = info else { return }
        apply(&next)
        info = next
        _ = model.session.setText(info: next)
        render()
    }

    /// Draws the text into its layer off the main thread (only where it was and now reaches), those
    /// asked for while one draws passed over for the latest.
    private func render() {
        guard info != nil else { return }
        let first = waiting.withLock {
            let first = !self.drawAsked
            self.drawAsked = true
            return first
        }
        guard first else { return }
        let session = model.session
        queue.async { [weak self] in
            guard let self else { return }
            self.waiting.withLock { self.drawAsked = false }
            TextFonts.wait()
            _ = session.drawTextIntoLayer()
        }
    }

    /// Ends editing text: `keep` makes it one undo step, otherwise it goes back.
    func end(keep: Bool) {
        guard info != nil else { return }
        info = nil
        styleOpen = false
        let session = model.session
        // the last drawing lands first
        queue.async { [weak self] in
            session.textEnd(keep: keep)
            DispatchQueue.main.async { self?.model.refresh() }
        }
    }

    /// Dropped without its work, undoing out of the edit.
    func drop() {
        info = nil
        styleOpen = false
    }

    func openStyle() {
        beforeStyle = info
        styleOpen = true
    }

    func closeStyle() {
        styleOpen = false
    }

    /// The style sheet's Cancel: its changes go back (the text, where it is and its width stay), and
    /// the keyboard returns.
    func cancelStyle() {
        if let before = beforeStyle {
            change { now in
                let kept = (now.text, now.x, now.y, now.width)
                now = before
                (now.text, now.x, now.y, now.width) = kept
            }
        }
        styleOpen = false
    }

    /// Copies the selected text (all of it if none is), and with `cut` takes it out.
    func copy(cut: Bool) {
        let text = field as NSString
        let range = selection.length == 0 ? NSRange(location: 0, length: text.length) : selection
        guard range.length > 0 else { return }
        UIPasteboard.general.string = text.substring(with: range)
        if cut {
            fieldChanged(text.replacingCharacters(in: range, with: ""), selection: NSRange(location: range.location, length: 0))
        }
    }

    /// Puts the copied text in place of the selection.
    func paste() {
        guard let add = UIPasteboard.general.string else { return }
        let text = (field as NSString).replacingCharacters(in: selection, with: add)
        fieldChanged(text, selection: NSRange(location: selection.location + (add as NSString).length, length: 0))
    }

    /// Keeps a font file and offers it; the text being edited takes it.
    func importFont(_ url: URL) {
        guard let bytes = url.readPicked(), let typeface = importTextFont(bytes: bytes, fileName: url.lastPathComponent) else { return }
        families = textTypefaces()
        change { $0.font = typeface.family }
        model.showNotice("Font imported")
    }

    /// Moves the text by a drag between two window points.
    func move(from: CGPoint, to: CGPoint) {
        let pixels = model.pixels
        guard let a = model.windowToCanvas(CGPoint(x: from.x * pixels, y: from.y * pixels)),
              let b = model.windowToCanvas(CGPoint(x: to.x * pixels, y: to.y * pixels)) else { return }
        change {
            $0.x += Float(b.x - a.x)
            $0.y += Float(b.y - a.y)
        }
    }

    func side(at p: CGPoint) -> TextSide? {
        let pixels = model.pixels
        return model.session.textSideAt(x: Float(p.x * pixels), y: Float(p.y * pixels), reach: Float(sideReach * pixels))
    }

    func beginSide(_ side: TextSide, from p: CGPoint) -> Bool {
        let pixels = model.pixels
        return model.session.textSideBegin(side: side, x: Float(p.x * pixels), y: Float(p.y * pixels))
    }

    /// The side being dragged follows the drag: the text wraps to the box's new width.
    func dragSide(to p: CGPoint) {
        let pixels = model.pixels
        guard let next = model.session.textSideDrag(x: Float(p.x * pixels), y: Float(p.y * pixels)) else { return }
        info = next
        render()
    }

    func endSide() {
        model.session.textSideEnd()
        model.refresh()
    }

    /// Inside editing text, a change to its box's width undone or redone, the text still edited;
    /// false if there is none.
    func stepBox(undo: Bool) -> Bool {
        let session = model.session
        guard let name = undo ? session.textBoxUndo() : session.textBoxRedo() else { return false }
        if let id = session.editingText(), let text = session.layerText(id: id) {
            info = text
        }
        render()
        model.showNotice((undo ? "Undo " : "Redo ") + name)
        model.refresh()
        return true
    }

    /// The canvas's dots per inch.
    var dpi: Float {
        max(model.session.info().dpi, 1)
    }

    /// The text's box on the canvas, pixels.
    func bounds(_ t: TextInfo) -> CGRect {
        TextFonts.wait()
        let b = textBox(info: t)
        guard b.count == 4 else { return .zero }
        return CGRect(x: CGFloat(b[0]), y: CGFloat(b[1]), width: CGFloat(b[2]), height: CGFloat(b[3]))
    }

    /// Where the caret before `index` (a UTF-16 place) stands on the canvas: x, its top and bottom;
    /// nil for vertical text.
    func caret(_ t: TextInfo, _ index: Int) -> [Float]? {
        TextFonts.wait()
        let at = textIndexFromUtf16(text: t.text, utf16: UInt32(max(index, 0)))
        let found = textCaret(info: t, index: at)
        return found.count == 3 ? found : nil
    }

    /// The letters from `start` to `end` (UTF-16 places) ringed in blue, as a picture of the canvas.
    func chosen(_ t: TextInfo, _ start: Int, _ end: Int) -> TextPicture? {
        TextFonts.wait()
        let from = textIndexFromUtf16(text: t.text, utf16: UInt32(max(start, 0)))
        let to = textIndexFromUtf16(text: t.text, utf16: UInt32(max(end, 0)))
        return drawChosenText(info: t, start: from, end: to, ring: boxLineArgb)
    }
}

/// The field the keyboard types into, out of sight; the bar above the keyboard rides on it.
private struct TextInputHost: UIViewRepresentable {
    let tool: TextTool

    final class Coordinator: NSObject, UITextViewDelegate {
        let tool: TextTool

        init(tool: TextTool) {
            self.tool = tool
        }

        func textViewDidChange(_ view: UITextView) {
            tool.fieldChanged(view.text, selection: view.selectedRange)
        }

        func textViewDidChangeSelection(_ view: UITextView) {
            tool.selectionChanged(view.selectedRange)
        }

        @objc func editStyle() {
            tool.openStyle()
        }
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(tool: tool)
    }

    private func bar(_ coordinator: Coordinator) -> UIView {
        let bar = UIView(frame: CGRect(x: 0, y: 0, width: 100, height: 44))
        bar.backgroundColor = UIColor(Look.shared.light ? Color(argb: 0xFFE9_ECED) : Color(argb: 0xFF2B_2B2B))
        let button = UIButton(type: .system)
        button.setTitle("Edit style", for: .normal)
        button.setTitleColor(UIColor(sheetInk), for: .normal)
        button.titleLabel?.font = .systemFont(ofSize: 15)
        button.addTarget(coordinator, action: #selector(Coordinator.editStyle), for: .touchUpInside)
        button.translatesAutoresizingMaskIntoConstraints = false
        bar.addSubview(button)
        NSLayoutConstraint.activate([
            button.trailingAnchor.constraint(equalTo: bar.trailingAnchor, constant: -24),
            button.centerYAnchor.constraint(equalTo: bar.centerYAnchor),
        ])
        return bar
    }

    func makeUIView(context: Context) -> UITextView {
        let view = UITextView(frame: CGRect(x: 0, y: 0, width: 1, height: 1))
        view.alpha = 0.01
        view.autocorrectionType = .no
        view.delegate = context.coordinator
        view.inputAccessoryView = bar(context.coordinator)
        return view
    }

    func updateUIView(_ view: UITextView, context: Context) {
        if view.text != tool.field {
            view.text = tool.field
        }
        if view.selectedRange != tool.selection {
            view.selectedRange = tool.selection
        }
        let wanted = tool.keyboard && !tool.styleOpen
        DispatchQueue.main.async {
            if wanted, !view.isFirstResponder {
                view.becomeFirstResponder()
            } else if !wanted, view.isFirstResponder {
                view.resignFirstResponder()
            }
        }
    }
}

/// Text being edited: its box drawn over the canvas, a hidden field the keyboard types into, and a
/// bar above the keyboard with the style sheet's button. Tapping outside the box keeps the text.
struct TextEditing: View {
    let model: CanvasModel
    let screen: CGSize

    /// What a touch took: a side's dot, the text, or nothing yet; how far it has gone.
    @State private var side: TextSide?
    @State private var touching = false
    @State private var outside = false
    @State private var dragging = false
    @State private var last = CGPoint.zero

    /// The text's box in the window, as the smallest upright rectangle around it.
    private func windowBox(_ tool: TextTool, _ info: TextInfo) -> CGRect {
        let corners = boxCorners(tool, info)
        let xs = corners.map(\.x)
        let ys = corners.map(\.y)
        guard let left = xs.min(), let right = xs.max(), let top = ys.min(), let bottom = ys.max() else { return .zero }
        return CGRect(x: left, y: top, width: right - left, height: bottom - top)
    }

    private func boxCorners(_ tool: TextTool, _ info: TextInfo) -> [CGPoint] {
        let b = tool.bounds(info)
        return [
            CGPoint(x: b.minX, y: b.minY), CGPoint(x: b.maxX, y: b.minY),
            CGPoint(x: b.maxX, y: b.maxY), CGPoint(x: b.minX, y: b.maxY),
        ].map { model.canvasToWindow($0) }
    }

    private func touched(_ drag: DragGesture.Value, _ tool: TextTool, _ info: TextInfo) {
        if !touching {
            touching = true
            dragging = false
            last = drag.startLocation
            side = tool.side(at: drag.startLocation)
            outside = side == nil && !windowBox(tool, info).contains(drag.startLocation)
        }
        if let side {
            if !dragging, hypot(drag.translation.width, drag.translation.height) > 8 {
                dragging = tool.beginSide(side, from: drag.startLocation)
            }
            if dragging {
                tool.dragSide(to: drag.location)
            }
        } else if !outside {
            // a touch on the text brings the keyboard back if it was put away
            tool.keyboard = true
            tool.move(from: last, to: drag.location)
            last = drag.location
        }
    }

    private func letGo(_ tool: TextTool) {
        touching = false
        if side != nil {
            if dragging {
                tool.endSide()
            } else {
                // a tap on the dot is a tap on the text
                tool.keyboard = true
            }
        } else if outside {
            // one finger there ends the edit, keeping it
            tool.end(keep: true)
        }
        side = nil
    }

    var body: some View {
        let tool = model.text
        if let info = tool.info {
            ZStack(alignment: .bottom) {
                Color.clear
                    .contentShape(Rectangle())
                    .gesture(
                        DragGesture(minimumDistance: 0)
                            .onChanged { touched($0, tool, info) }
                            .onEnded { _ in letGo(tool) }
                    )
                TextBox(model: model, tool: tool, info: info, corners: boxCorners(tool, info))
                TextInputHost(tool: tool)
                    .frame(width: 1, height: 1)
                    .opacity(0.01)
                    .allowsHitTesting(false)
                if tool.styleOpen {
                    TextStyleSheet(tool: tool, info: info, screen: screen)
                }
            }
        }
    }
}

/// The text's box: a thin blue line around where it is drawn, a dot at the middle of each side
/// (dragged, its width), the caret, or the chosen letters ringed with a handle at each end.
private struct TextBox: View {
    let model: CanvasModel
    let tool: TextTool
    let info: TextInfo
    let corners: [CGPoint]

    var body: some View {
        let length = (info.text as NSString).length
        let start = min(max(tool.selection.location, 0), length)
        let end = min(start + tool.selection.length, length)
        let caretStart = tool.caret(info, start)
        let caretEnd = tool.caret(info, end)
        let picture = info.vertical || start == end ? nil : tool.chosen(info, start, end)
        let ringed = picture.flatMap { p in
            CanvasModel.image(ThumbnailImage(width: p.width, height: p.height, rgba: p.rgba)).map { (Image(uiImage: $0), p) }
        }
        let view = model.viewMatrix.map { CGFloat($0) }
        let pixels = model.pixels
        Canvas { context, _ in
            var path = Path()
            path.addLines(corners)
            path.closeSubpath()
            context.stroke(path, with: .color(boxOutline), lineWidth: 1)
            let middles = [
                CGPoint(x: (corners[0].x + corners[3].x) / 2, y: (corners[0].y + corners[3].y) / 2),
                CGPoint(x: (corners[1].x + corners[2].x) / 2, y: (corners[1].y + corners[2].y) / 2),
            ]
            for at in middles {
                let r = sideDot / 2
                context.fill(Path(ellipseIn: CGRect(x: at.x - r, y: at.y - r, width: r * 2, height: r * 2)), with: .color(boxOutline))
            }
            // vertical text spreads its letters apart, so it shows none of this
            guard !info.vertical else { return }
            func spot(_ caret: [Float]?, bottom: Bool) -> CGPoint? {
                caret.map { model.canvasToWindow(CGPoint(x: CGFloat($0[0]), y: CGFloat(bottom ? $0[2] : $0[1]))) }
            }
            func bar(_ a: CGPoint, _ b: CGPoint) {
                var line = Path()
                line.move(to: a)
                line.addLine(to: b)
                context.stroke(line, with: .color(boxLine), lineWidth: 2)
            }
            if start == end {
                if let top = spot(caretStart, bottom: false), let bottom = spot(caretStart, bottom: true) {
                    bar(top, bottom)
                }
                return
            }
            if let (image, p) = ringed, view.count == 6 {
                var canvas = context
                canvas.scaleBy(x: 1 / pixels, y: 1 / pixels)
                canvas.concatenate(CGAffineTransform(a: view[0], b: view[1], c: view[2], d: view[3], tx: view[4], ty: view[5]))
                canvas.draw(image, in: CGRect(x: CGFloat(p.x), y: CGFloat(p.y), width: CGFloat(p.width), height: CGFloat(p.height)))
            }
            guard let top = spot(caretStart, bottom: false), let bottom = spot(caretEnd, bottom: true),
                  let startFoot = spot(caretStart, bottom: true), let endTop = spot(caretEnd, bottom: false) else { return }
            bar(top, startFoot)
            bar(endTop, bottom)
            for at in [top, bottom] {
                context.fill(Path(ellipseIn: CGRect(x: at.x - 6, y: at.y - 6, width: 12, height: 12)), with: .color(boxLine))
            }
        }
        .allowsHitTesting(false)
    }
}

/// The style sheet in the keyboard's place: a bar (the keyboard back, cut, copy and paste, import a
/// font, Cancel, Done); then Typeface (fonts and styles, each name in its own face) or Design (size,
/// spacing, opacity, and the attributes), switched at the bottom.
private struct TextStyleSheet: View {
    let tool: TextTool
    let info: TextInfo
    let screen: CGSize

    @State private var page = 0
    @State private var fontAsked = false

    private func sheetIcon(_ icon: PcIcon, size: CGFloat, tap: @escaping () -> Void) -> some View {
        PcIconView(icon, size: size, color: capsuleInk)
            .frame(width: 41, height: 40)
            .contentShape(Rectangle())
            .onTapGesture(perform: tap)
    }

    private var bar: some View {
        HStack(spacing: 13) {
            PcIconView(PcIcons.keyboard, size: 40, color: capsuleInk)
                .frame(width: 44, height: 44)
                .background(Circle().fill(keyboardDisc))
                .contentShape(Circle())
                .onTapGesture {
                    tool.closeStyle()
                    tool.keyboard = true
                }
            HStack(spacing: 7) {
                sheetIcon(PcIcons.textCut, size: 40) { tool.copy(cut: true) }
                sheetIcon(PcIcons.textCopy, size: 40) { tool.copy(cut: false) }
                sheetIcon(PcIcons.textPaste, size: 40) { tool.paste() }
            }
            .background(RoundedRectangle(cornerRadius: 12, style: .circular).fill(sheetCapsule))
            Spacer(minLength: 0)
            HStack(spacing: 0) {
                word("Import") { fontAsked = true }
                word("Cancel") { tool.cancelStyle() }
            }
            .frame(height: 43)
            .background(Capsule().fill(sheetCapsule))
            PcIconView(PcIcons.doneTick, size: 40, color: .white)
                .frame(width: 44, height: 45)
                .background(Capsule().fill(doneBlue))
                .contentShape(Capsule())
                .onTapGesture { tool.end(keep: true) }
        }
        .padding(.horizontal, 8)
        .frame(height: 62)
        .background(sheetBar)
    }

    private func word(_ label: String, tap: @escaping () -> Void) -> some View {
        Text(verbatim: label)
            .font(.system(size: 15))
            .foregroundStyle(capsuleInk)
            .padding(.horizontal, 14)
            .frame(maxHeight: .infinity)
            .contentShape(Rectangle())
            .onTapGesture(perform: tap)
    }

    /// Typeface | Design, when the cards don't all fit.
    private var switcher: some View {
        HStack(spacing: 12) {
            ForEach(0 ..< 2, id: \.self) { i in
                Text(verbatim: i == 0 ? "Typeface" : "Design")
                    .font(.system(size: 13))
                    .foregroundStyle(i == page ? Color.white : capsuleInk)
                    .frame(width: 156, height: 30)
                    .background(Capsule().fill(i == page ? Pc.accent : Color.clear))
                    .contentShape(Capsule())
                    .onTapGesture { page = i }
            }
        }
        .padding(2)
        .frame(height: 34)
        .background(Capsule().fill(segmentTrack))
        .padding(.top, 11)
        .frame(maxWidth: .infinity)
        .frame(height: 81, alignment: .top)
    }

    private func rows(_ count: Int, _ cell: @escaping (Int) -> Cell) -> some View {
        ScrollView(.vertical, showsIndicators: false) {
            VStack(spacing: 0) {
                ForEach(0 ..< count, id: \.self) { cell($0) }
            }
            .padding(.top, 6)
        }
        .padding(.leading, 10)
        .padding(.trailing, 17)
    }

    private var fontCard: some View {
        SheetCard(title: "Font", shape: UnevenRoundedRectangle(topLeadingRadius: cardCorner, bottomLeadingRadius: cardCorner, style: .circular)) {
            rows(tool.families.count) { i in
                let f = tool.families[i]
                return Cell(name: f.name, selected: f.family == info.font, font: .custom(f.name, size: 15)) {
                    tool.change { $0.font = f.family }
                }
            }
        }
    }

    private var styleCard: some View {
        SheetCard(title: "Style", shape: UnevenRoundedRectangle(bottomTrailingRadius: cardCorner, topTrailingRadius: cardCorner, style: .circular)) {
            rows(tool.styles.count) { i in
                let s = tool.styles[i]
                return Cell(name: s, selected: s == info.style, font: .system(size: 15)) {
                    tool.change { $0.style = s }
                }
            }
        }
    }

    private func pt(_ px: Float) -> Float {
        px * 72 / tool.dpi
    }

    private var designCard: some View {
        SheetCard(title: "Design", shape: RoundedRectangle(cornerRadius: cardCorner, style: .circular)) {
            VStack(spacing: 0) {
                DesignRow(label: "Size", value: sizeToSlider(info.size), shown: String(format: "%.1fpt", pt(info.size)), fromMiddle: false) { v in
                    tool.change { $0.size = sliderToSize(v) }
                }
                DesignRow(label: "Kerning", value: info.kerning / 2 + 0.5, shown: String(format: "%.1f%%", info.kerning * 100)) { v in
                    tool.change { $0.kerning = v * 2 - 1 }
                }
                DesignRow(label: "Tracking", value: info.tracking / 2 + 0.5, shown: String(format: "%.1f%%", info.tracking * 100)) { v in
                    tool.change { $0.tracking = v * 2 - 1 }
                }
                DesignRow(label: "Leading", value: info.leading / 2 + 0.5, shown: "\(wholeNumber(pt(info.leading * info.size)))pt") { v in
                    tool.change { $0.leading = v * 2 - 1 }
                }
                DesignRow(label: "Baseline", value: info.baseline / 2 + 0.5, shown: "\(wholeNumber(pt(info.baseline * info.size)))pt") { v in
                    tool.change { $0.baseline = v * 2 - 1 }
                }
                DesignRow(label: "Opacity", value: info.opacity, shown: String(format: "%.1f%%", info.opacity * 100), fromMiddle: false) { v in
                    tool.change { $0.opacity = v }
                }
            }
            .padding(.horizontal, 4)
        }
    }

    private func attribute(_ icon: PcIcon, on: Bool, size: CGFloat, tall: CGFloat, tap: @escaping () -> Void) -> some View {
        PcIconView(icon, size: size, color: on ? .white : sheetInk)
            .frame(maxWidth: .infinity)
            .frame(height: tall)
            .background(RoundedRectangle(cornerRadius: 8, style: .circular).fill(on ? Pc.accent : buttonFill))
            .contentShape(Rectangle())
            .onTapGesture(perform: tap)
    }

    private func attributesCard(wide: Bool) -> some View {
        let aligns: [(TextAlignment, PcIcon)] = [
            (.left, PcIcons.alignLeft), (.center, PcIcons.alignCenter), (.right, PcIcons.alignRight), (.justify, PcIcons.alignJustify),
        ]
        let gridTall: CGFloat = wide ? 36 : 35
        let perRow = wide ? 4 : 2
        return SheetCard(title: "Attributes", shape: RoundedRectangle(cornerRadius: cardCorner, style: .circular)) {
            VStack(spacing: wide ? 20 : 4) {
                ForEach(Array(stride(from: 0, to: aligns.count, by: perRow)), id: \.self) { first in
                    HStack(spacing: 4) {
                        ForEach(first ..< first + perRow, id: \.self) { i in
                            attribute(aligns[i].1, on: info.align == aligns[i].0, size: 32, tall: gridTall) {
                                tool.change { $0.align = aligns[i].0 }
                            }
                        }
                    }
                }
                let looks = HStackOrStack(wide: wide) {
                    attribute(PcIcons.textUnderline, on: info.underline, size: 38, tall: 36) { tool.change { $0.underline.toggle() } }
                    attribute(PcIcons.textOutline, on: info.outline, size: 38, tall: 36) { tool.change { $0.outline.toggle() } }
                    attribute(PcIcons.textVertical, on: info.vertical, size: 41, tall: 36) { tool.change { $0.vertical.toggle() } }
                }
                looks
                HStack(spacing: 0) {
                    // two serif capitals all but touching
                    Text(verbatim: "TT")
                        .font(.system(size: 26.5, design: .serif))
                        .tracking(-0.5)
                        .foregroundStyle(sheetInk)
                    Spacer(minLength: 0)
                    PcSwitch(on: info.allCaps)
                }
                .contentShape(Rectangle())
                .onTapGesture { tool.change { $0.allCaps.toggle() } }
                .padding(.top, wide ? 4 : 0)
            }
            .padding(.horizontal, wide ? 10 : 6.5)
            .padding(.top, wide ? 6 : 7)
        }
    }

    var body: some View {
        let wide = screen.width >= wideSheet
        VStack(spacing: 0) {
            bar
            // wide (landscape), the four cards side by side; narrow, Typeface or Design, switched below
            GeometryReader { place in
                let room = place.size.width - 20
                HStack(spacing: 0) {
                    if wide {
                        let total: CGFloat = 263.5 + 184.5 + 328 + 325
                        let left = room - 12
                        fontCard.frame(width: left * 263.5 / total)
                        Spacer().frame(width: 2)
                        styleCard.frame(width: left * 184.5 / total)
                        Spacer().frame(width: 5)
                        designCard.frame(width: left * 328 / total)
                        Spacer().frame(width: 5)
                        attributesCard(wide: true).frame(width: left * 325 / total)
                    } else if page == 0 {
                        fontCard.frame(width: (room - 2) * 424.5 / 722)
                        Spacer().frame(width: 2)
                        styleCard.frame(width: (room - 2) * 297.5 / 722)
                    } else {
                        designCard.frame(width: (room - 5) * 584 / 719)
                        Spacer().frame(width: 5)
                        attributesCard(wide: false).frame(width: (room - 5) * 135 / 719)
                    }
                }
                .padding(.horizontal, 10)
            }
            if wide {
                Spacer().frame(height: 33)
            } else {
                switcher
            }
        }
        .frame(height: wide ? 521 : 561)
        .background(sheetFill)
        // a touch on the sheet between its controls stays in it: it doesn't end the editing
        .contentShape(Rectangle())
        .onTapGesture {}
        .fileImporter(isPresented: $fontAsked, allowedContentTypes: [.font]) { result in
            if case let .success(url) = result {
                tool.importFont(url)
            }
        }
    }
}

/// Views in a row, or one under another.
private struct HStackOrStack<Content: View>: View {
    let wide: Bool
    @ViewBuilder let content: () -> Content

    var body: some View {
        if wide {
            HStack(spacing: 4) { content() }
        } else {
            VStack(spacing: 4) { content() }
        }
    }
}

/// A card of the sheet: its title over what it holds.
private struct SheetCard<Outline: Shape, Content: View>: View {
    let title: String
    let shape: Outline
    @ViewBuilder let content: () -> Content

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(verbatim: title)
                .font(.system(size: 17, weight: .semibold))
                .foregroundStyle(sheetInk)
                .padding(.leading, 20)
                .padding(.top, 16.5)
                .padding(.bottom, 10)
            content()
            Spacer(minLength: 0)
        }
        .frame(maxHeight: .infinity)
        .background(shape.fill(cardFill))
        .clipShape(shape)
    }
}

/// A font or style: its name drawn in that face, the chosen one a blue cell.
private struct Cell: View {
    let name: String
    let selected: Bool
    let font: Font
    let tap: () -> Void

    var body: some View {
        Text(verbatim: name)
            .font(font)
            .foregroundStyle(selected ? Color.white : sheetInk)
            .lineLimit(1)
            .padding(.leading, 10)
            .frame(maxWidth: .infinity, minHeight: 33, maxHeight: 33, alignment: .leading)
            .background(selected ? Pc.accent : Color.clear)
            .contentShape(Rectangle())
            .onTapGesture(perform: tap)
    }
}

/// A design setting: its name, its value in a pill, and a thin bar under them (filled from the left,
/// or a tick in the middle).
private struct DesignRow: View {
    let label: String
    let value: Float
    let shown: String
    var fromMiddle = true
    let onChange: (Float) -> Void

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                Text(verbatim: label)
                    .font(.system(size: 15))
                    .foregroundStyle(sheetInk)
                Spacer(minLength: 0)
                Text(verbatim: shown)
                    .font(.system(size: 13))
                    .foregroundStyle(sheetInk)
                    .padding(.horizontal, 10)
                    .frame(height: 24)
                    .background(Capsule().fill(buttonFill))
            }
            .padding(.top, 1.5)
            GeometryReader { g in
                let w = g.size.width
                let v = CGFloat(min(max(value, 0), 1))
                let x = min(max(w * v, 8), max(w - 8, 8))
                ZStack(alignment: .leading) {
                    Capsule().fill(designTrack).frame(height: 2)
                    if fromMiddle {
                        Rectangle().fill(designTick).frame(width: 1, height: 10).offset(x: w / 2)
                    } else {
                        Capsule().fill(Pc.accent).frame(width: w * v, height: 2)
                    }
                    Capsule().fill(Color.white).frame(width: 16, height: 10).offset(x: x - 8)
                }
                .frame(width: w, height: 20)
                .contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 0).onChanged { drag in
                    onChange(Float(min(max(drag.location.x / max(w, 1), 0), 1)))
                })
            }
            .frame(height: 20)
        }
        .padding(.horizontal, 16)
        .frame(height: 58.5)
    }
}
