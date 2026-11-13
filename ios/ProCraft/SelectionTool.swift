import SwiftUI
import UIKit

/// The selection toolbar's buttons: with room (landscape), and where the screen is narrow.
private let buttonRoomy: CGFloat = 83
private let buttonNarrow: CGFloat = 70
private let featherWidth: CGFloat = 250
private let startDot = Color(argb: 0xFFC7_C7C7)
private let savedThumb = Color(argb: 0xFF00_0000)
/// A path's dashes, dark over light.
let overlayDash = Color(argb: 0xE600_0000)
let overlayDashGap = Color(argb: 0xE6FF_FFFF)

/// How the selection tool selects.
enum SelectionKind: String, CaseIterable {
    case automatic, freehand, rectangle, ellipse

    var label: String {
        switch self {
        case .automatic: "Automatic"
        case .freehand: "Freehand"
        case .rectangle: "Rectangle"
        case .ellipse: "Ellipse"
        }
    }

    fileprivate var icon: PcIcon {
        switch self {
        case .automatic: PcIcons.automatic
        case .freehand: PcIcons.freehand
        case .rectangle: PcIcons.rectangleShape
        case .ellipse: PcIcons.ellipseShape
        }
    }

    fileprivate var touch: SelectKind {
        switch self {
        case .automatic: .automatic
        case .freehand: .freehand
        case .rectangle: .rectangle
        case .ellipse: .ellipse
        }
    }
}

/// A popover over the selection toolbar.
enum SelectionPopup {
    case none, feather, saved
}

/// The selection tool's state: the core keeps the selection, this what the tool shows of it.
@Observable
final class SelectionTool {
    private static let kindKey = "selectionKind"
    private static let thresholdKey = "selectionThreshold"

    @ObservationIgnored private unowned let model: CanvasModel

    private(set) var kind: SelectionKind
    /// New shapes take away from the selection instead of adding to it.
    private(set) var removes = false
    /// Each new selection is filled with the paint colour.
    private(set) var colorFill = false
    private(set) var hasSelection = false
    private(set) var popup = SelectionPopup.none
    /// The feather popover's amount (0..1).
    private(set) var featherAmount: Float = 0
    /// How many selections are kept, and their pictures.
    private(set) var saved = 0
    private(set) var savedThumbnails: [Int: UIImage] = [:]
    /// The freehand path as drawn (canvas pixels).
    private(set) var freehand: [CGPoint] = []
    /// A rectangle or ellipse being dragged out, between two window points.
    private(set) var shapeDrag: (CGPoint, CGPoint)?
    /// A colour selection's threshold being slid: from where (window points), and how far.
    private(set) var thresholdDrag: (at: CGPoint, threshold: Float)?
    /// Where a colour selection's threshold starts.
    private(set) var threshold: Float
    /// While the tool is open on a plugin's layer: the box round the strokes the selection takes
    /// (four corners, x then y, canvas pixels), else empty.
    private(set) var takenBox: [Float] = []

    /// A feather's work off the main thread, the newest amount when it gets there.
    @ObservationIgnored private let queue = DispatchQueue(label: "feather", qos: .userInitiated)
    @ObservationIgnored private let waiting = NSLock()
    @ObservationIgnored private var nextFeather: Float?

    init(model: CanvasModel) {
        self.model = model
        let d = UserDefaults.standard
        kind = d.string(forKey: Self.kindKey).flatMap(SelectionKind.init(rawValue:)) ?? .freehand
        threshold = d.object(forKey: Self.thresholdKey) as? Float ?? 0.5
    }

    /// Each time the tool opens, shapes add to the selection.
    func opened() {
        removes = false
    }

    /// The tool closes: a path being drawn becomes a selection.
    func closed() {
        closeFreehand()
        setPopup(.none)
    }

    func choose(_ next: SelectionKind) {
        closeFreehand()
        kind = next
        UserDefaults.standard.set(next.rawValue, forKey: Self.kindKey)
        showLook()
    }

    func chooseRemoves(_ value: Bool) {
        removes = value
    }

    /// How a selection joins the one there is: selecting by colour always adds (Remove greyed).
    private var join: SelectionJoin {
        removes && kind != .automatic ? .remove : .add
    }

    /// How the canvas's touches select while the tool is open; none otherwise.
    func touch() -> SelectTouch? {
        guard model.selecting else { return nil }
        return SelectTouch(kind: kind.touch, join: join, threshold: threshold)
    }

    /// The freehand path as the touches have drawn it.
    func freehandChanged() {
        let flat = model.session.freehandPath()
        freehand = stride(from: 0, to: flat.count - 1, by: 2).compactMap {
            model.windowToCanvas(CGPoint(x: CGFloat(flat[$0]), y: CGFloat(flat[$0 + 1])))
        }
    }

    /// The touches' rectangle or ellipse, dragged out between two points (window pixels).
    func dragged(_ a: CGPoint, _ b: CGPoint) {
        let pixels = model.pixels
        shapeDrag = (CGPoint(x: a.x / pixels, y: a.y / pixels), CGPoint(x: b.x / pixels, y: b.y / pixels))
    }

    /// The touches made a selection (or a shape dragged out ended): Color Fill fills it.
    func touched() {
        shapeDrag = nil
        changed()
    }

    /// A colour selection's threshold slid from window pixels `p`: shown as it goes; `done`, kept
    /// for the next one.
    func colourSlid(_ p: CGPoint, threshold value: Float, done: Bool) {
        if !done {
            let session = model.session
            thresholdDrag = (CGPoint(x: p.x / model.pixels, y: p.y / model.pixels), value)
            session.setSelectionDisplay(display: .fill)
            session.selectByColour(x: Float(p.x), y: Float(p.y), threshold: value, join: join, live: true)
            changed(fill: false)
            return
        }
        // the touches made the selection as it was let go
        thresholdDrag = nil
        threshold = value
        UserDefaults.standard.set(value, forKey: Self.thresholdKey)
    }

    /// Closes the freehand path into a selection (a path of fewer than three points is dropped).
    func closeFreehand() {
        let drawn = !freehand.isEmpty
        freehand = []
        model.session.closeFreehand()
        if drawn {
            changed()
        }
    }

    func invert() {
        model.session.selectInvert()
        changed(fill: false)
    }

    func copyToLayer() {
        guard model.session.copySelectionToLayer() != nil else { return }
        model.session.selectNone()
        changed(fill: false)
        model.refresh()
    }

    /// The feather popover's amount: the selection's edges softened from how they were before.
    func featherTo(_ amount: Float) {
        featherAmount = amount
        let session = model.session
        let first = waiting.withLock {
            let first = nextFeather == nil
            nextFeather = amount
            return first
        }
        guard first else { return }
        queue.async { [weak self] in
            guard let self else { return }
            let latest = waiting.withLock {
                let latest = self.nextFeather
                self.nextFeather = nil
                return latest
            }
            if let latest {
                session.selectFeather(amount: latest)
            }
        }
    }

    /// Opens `next`, or closes it if it is open.
    func openPopup(_ next: SelectionPopup) {
        setPopup(popup == next ? .none : next)
        if popup == .feather {
            featherAmount = 0
        }
        saved = Int(model.session.savedSelectionCount())
        if popup == .saved {
            loadThumbnails()
        }
    }

    /// The popover changes; a feather being left is finished.
    func setPopup(_ next: SelectionPopup) {
        if popup == .feather, next != .feather {
            let session = model.session
            // after the feathering still on its way
            queue.sync {
                session.selectFeatherDone()
            }
            model.refresh()
        }
        popup = next
    }

    func save() {
        saved = Int(model.session.saveSelection())
        loadThumbnails()
    }

    private func loadThumbnails() {
        savedThumbnails = [:]
        for i in 0 ..< saved {
            if let t = model.session.savedSelectionThumbnail(index: UInt32(i), maxWidth: 128, maxHeight: 92) {
                savedThumbnails[i] = CanvasModel.image(t)
            }
        }
    }

    func load(_ i: Int) {
        _ = model.session.loadSelection(index: UInt32(i))
        changed(fill: false)
    }

    func toggleColorFill() {
        colorFill.toggle()
        if colorFill, hasSelection {
            model.session.fillSelection()
            model.refresh()
        }
    }

    func clear() {
        closeFreehand()
        model.session.selectNone()
        changed(fill: false)
    }

    private func changed(fill: Bool = true) {
        let session = model.session
        hasSelection = session.hasSelection()
        if fill, colorFill, hasSelection {
            session.fillSelection()
            model.refresh()
        }
        showLook()
    }

    /// Bands over what is not selected, except while selecting by colour, which tints what is.
    func showLook() {
        let tint = model.selecting && kind == .automatic
        model.session.setSelectionDisplay(display: tint ? .tint : .stripes)
        refreshTaken()
    }

    /// The box round the strokes the selection takes, read again while the tool is open.
    func refreshTaken() {
        let session = model.session
        let box = model.selecting && session.hasSelection() ? session.takenPluginItemsBox() : []
        if box != takenBox {
            takenBox = box
        }
    }
}

/// The selection tool's floating toolbar: its four ways, and what can be done with the selection.
struct SelectionToolbar: View {
    let model: CanvasModel
    let screenWidth: CGFloat

    var body: some View {
        let tool = model.selection
        let has = tool.hasSelection
        let drawing = !tool.freehand.isEmpty
        // selecting by colour always adds: Add shows chosen, and only the other ways can remove
        let joins = tool.kind != .automatic
        FloatingToolbar(
            layout: ToolbarLayout(screenWidth: screenWidth, count: 8, roomy: buttonRoomy, narrow: buttonNarrow),
            onBackground: { tool.setPopup(.none) }
        ) {
            ForEach(SelectionKind.allCases, id: \.self) { kind in
                // drawn in a 44 frame, shown 41 across
                ToolbarSegment(icon: kind.icon, label: kind.label, selected: tool.kind == kind, iconSize: 41) {
                    tool.choose(kind)
                }
            }
        } buttons: { w in
            // a long name smaller only where the buttons are narrow
            let long: CGFloat = w < buttonRoomy ? 11 : 12.5
            ToolbarButton(icon: PcIcons.selectAdd, label: "Add", active: !joins || !tool.removes, width: w, iconSize: 44) {
                if joins {
                    tool.chooseRemoves(false)
                }
            }
            ToolbarButton(icon: PcIcons.selectRemove, label: "Remove", active: joins && tool.removes, enabled: joins, width: w, iconSize: 44) {
                tool.chooseRemoves(true)
            }
            ToolbarButton(icon: PcIcons.invert, label: "Invert", enabled: has, width: w, iconSize: 44) { tool.invert() }
            ToolbarButton(icon: PcIcons.selectCopyPaste, label: "Copy & Paste", enabled: has || drawing, width: w, iconSize: 44, labelSize: long) {
                tool.copyToLayer()
            }
            ToolbarButton(icon: PcIcons.feather, label: "Feather", active: tool.popup == .feather, enabled: has || drawing, width: w, iconSize: 44) {
                tool.openPopup(.feather)
            }
            ToolbarButton(icon: PcIcons.saveLoad, label: "Save & Load", active: tool.popup == .saved, enabled: has || drawing, width: w, iconSize: 44, labelSize: long) {
                tool.openPopup(.saved)
            }
            ToolbarButton(icon: PcIcons.colorFill, label: "Color Fill", active: tool.colorFill, width: w, iconSize: 37) { tool.toggleColorFill() }
            ToolbarButton(icon: PcIcons.clearSelection, label: "Clear", enabled: has || drawing, width: w, iconSize: 44) { tool.clear() }
        }
    }
}

/// A popover over the selection toolbar: feathering the selection, or the selections kept.
struct SelectionPopover: View {
    let model: CanvasModel
    let screen: CGSize

    var body: some View {
        let tool = model.selection
        if tool.popup != .none {
            let width = tool.popup == .feather ? featherWidth : toolbarMenuWidth
            let layout = ToolbarLayout(screenWidth: screen.width, count: 8, roomy: buttonRoomy, narrow: buttonNarrow)
            let centre = layout.buttonCentre(tool.popup == .feather ? 4 : 5, screenWidth: screen.width)
            let left = min(max(centre - width / 2, 0), screen.width - width)
            ToolbarPopover(
                arrowX: centre - left, left: left, bottom: ToolbarLayout.buttonsTop(screenHeight: screen.height),
                width: width, screenWidth: screen.width
            ) {
                if tool.popup == .feather {
                    ToolbarMenuTitle(text: "Feather")
                        .padding(.bottom, 4)
                    ToolbarMenuGroup {
                        let amount = tool.featherAmount
                        PanelSlider(
                            label: "Amount", value: amount,
                            shown: amount <= 0 ? "None" : "\(wholeNumber(amount * 100))%",
                            onChange: { tool.featherTo($0) }
                        )
                    }
                } else {
                    SavedSelections(tool: tool)
                }
            }
        }
    }
}

private struct SavedSelections: View {
    let tool: SelectionTool

    var body: some View {
        let corner = toolbarMenuCorner
        HStack(spacing: 0) {
            Text(verbatim: "Selections")
                .font(.system(size: 24, weight: .bold))
                .tracking(-0.015 * 24)
                .foregroundStyle(ToolbarInk.menuText)
                .lineLimit(1)
            Spacer(minLength: 0)
            PcIconView(PcIcons.add, size: 40, color: ToolbarInk.menuText)
                .frame(width: 40, height: 36)
                .contentShape(Rectangle())
                .onTapGesture { tool.save() }
        }
        .padding(.leading, 14)
        .frame(height: 40)
        ScrollView(.vertical, showsIndicators: false) {
            VStack(spacing: 1) {
                ForEach(0 ..< tool.saved, id: \.self) { i in
                    HStack(spacing: 14) {
                        ZStack {
                            savedThumb
                            if let picture = tool.savedThumbnails[i] {
                                Image(uiImage: picture)
                                    .resizable()
                                    .scaledToFit()
                            }
                        }
                        .frame(width: 64, height: 46)
                        .clipShape(UnevenRoundedRectangle(topLeadingRadius: corner, bottomLeadingRadius: corner, style: .circular))
                        ToolbarMenuLabel(text: "Selection \(i + 1)")
                        Spacer(minLength: 0)
                    }
                    .frame(height: 51)
                    .background(RoundedRectangle(cornerRadius: corner, style: .circular).fill(ToolbarInk.menuGroup))
                    .contentShape(Rectangle())
                    .onTapGesture { tool.load(i) }
                }
            }
        }
        .frame(height: 172)
        .padding(.top, 8)
    }
}

/// What the selection tool shows while a shape is made: the rectangle or ellipse being dragged and
/// the freehand path (dashed, its first point a dot to close it on), and the bar and crosshair while
/// a colour selection's threshold is dragged.
struct SelectionOverlay: View {
    let model: CanvasModel

    /// A path drawn as the selection's outline: thin dark dashes over light ones.
    private func dashed(_ context: GraphicsContext, _ path: Path) {
        let dash: [CGFloat] = [5, 5]
        context.stroke(path, with: .color(overlayDashGap), style: StrokeStyle(lineWidth: 0.75, dash: dash, dashPhase: 5))
        context.stroke(path, with: .color(overlayDash), style: StrokeStyle(lineWidth: 0.75, dash: dash, dashPhase: 0))
    }

    var body: some View {
        let tool = model.selection
        let path = tool.freehand
        let drag = tool.shapeDrag
        let slid = tool.thresholdDrag
        let kind = tool.kind
        let taken = tool.takenBox
        if drag != nil || !path.isEmpty || slid != nil || taken.count == 8 {
            let points = path.map { model.canvasToWindow($0) }
            ZStack(alignment: .top) {
                Canvas { context, _ in
                    if taken.count == 8 {
                        drawTakenBox(context, taken)
                    }
                    if let (a, b) = drag {
                        let r = CGRect(x: min(a.x, b.x), y: min(a.y, b.y), width: abs(a.x - b.x), height: abs(a.y - b.y))
                        dashed(context, kind == .ellipse ? Path(ellipseIn: r) : Path(r))
                    }
                    if let first = points.first {
                        var line = Path()
                        line.addLines(points)
                        dashed(context, line)
                        context.fill(
                            Path(ellipseIn: CGRect(x: first.x - 5.5, y: first.y - 5.5, width: 11, height: 11)),
                            with: .color(startDot)
                        )
                    }
                    if let p = slid?.at {
                        // a thin cross where the colour is picked
                        var cross = Path()
                        cross.move(to: CGPoint(x: p.x - 8, y: p.y))
                        cross.addLine(to: CGPoint(x: p.x + 8, y: p.y))
                        cross.move(to: CGPoint(x: p.x, y: p.y - 8))
                        cross.addLine(to: CGPoint(x: p.x, y: p.y + 8))
                        context.stroke(cross, with: .color(overlayDash), lineWidth: 1)
                    }
                }
                if let t = slid?.threshold {
                    Text(verbatim: "Selection Threshold \(percent(t))")
                        .font(.system(size: 14))
                        .foregroundStyle(Pc.messageText)
                        .frame(maxWidth: .infinity, minHeight: 24, maxHeight: 24)
                        .background(Pc.messageBar)
                        .offset(y: Pc.barHeight)
                }
            }
            .allowsHitTesting(false)
        }
    }

    /// The strokes the selection takes on a plugin's layer: their box, as the transform box shows
    /// it (dashed edges, a node at each corner).
    private func drawTakenBox(_ context: GraphicsContext, _ box: [Float]) {
        let corners = (0 ..< 4).map { model.canvasToWindow(CGPoint(x: CGFloat(box[2 * $0]), y: CGFloat(box[2 * $0 + 1]))) }
        var edge = Path()
        edge.addLines(corners)
        edge.closeSubpath()
        let dash: [CGFloat] = [5, 5]
        context.stroke(edge, with: .color(overlayDashGap), style: StrokeStyle(lineWidth: 1, dash: dash, dashPhase: 5))
        context.stroke(edge, with: .color(overlayDash), style: StrokeStyle(lineWidth: 1, dash: dash, dashPhase: 0))
        for c in corners {
            node(context, at: c, color: nodeBlue, scale: 1)
        }
    }

    /// A threshold (0..1) as a percentage with one decimal.
    private func percent(_ t: Float) -> String {
        let tenths = Int((t * 1000).rounded())
        return "\(tenths / 10).\(tenths % 10)%"
    }
}
