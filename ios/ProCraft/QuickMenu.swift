import SwiftUI

// glass: the buttons a dark tint over the canvas with a faint rim, the middle one a light tint
private let buttonFill = Color(argb: 0x9C28_2828)
private let buttonEdge = Color(argb: 0x40FF_FFFF)
private let buttonInk = Color(argb: 0xFFF7_F7F7)
private let titleFill = Color(argb: 0x94CF_CFCF)
private let titleInk = Color(argb: 0xFF0F_0F0F)
private let ringWidth: CGFloat = 171
private let ringHeight: CGFloat = 48
/// Where the buttons' centres are from the middle, clockwise from the top.
private let ringSlots: [CGPoint] = [
    CGPoint(x: 0, y: -69), CGPoint(x: 191, y: -39), CGPoint(x: 191, y: 39),
    CGPoint(x: 0, y: 69), CGPoint(x: -191, y: 39), CGPoint(x: -191, y: -39),
]
private let ringReachX: CGFloat = 191
private let ringReachY: CGFloat = 69

// the picker (as measured): a dark panel 220 wide, corners 30, an arrow 14 tall; capsule rows 47
// tall on a 50 pitch, 8.5 in from its sides, under a 55.5 tall title
private let pickerWidth: CGFloat = 220
private let pickerCorner: CGFloat = 30
private let pickerArrow: CGFloat = 14
private let pickerHeader: CGFloat = 55.5
private let pickerRowHeight: CGFloat = 47
private let pickerRowGap: CGFloat = 3
private let pickerRowPitch = pickerRowHeight + pickerRowGap
private let pickerInset: CGFloat = 8.5
private let pickerHeight = pickerHeader + pickerRowPitch * 6 - pickerRowGap + 6
private let pickerMargin: CGFloat = 8
/// How far below the list's top the chosen row shows.
private let pickerChosenAt: CGFloat = 89
private let pickerFill = Color(argb: 0xFF37_3737)
private let pickerRowFill = Color(argb: 0xFF2E_2E2E)
private let pickerRowInk = Color(argb: 0xFFC5_C5C5)
private let pickerTitleInk = Color(argb: 0xFFC4_C4C4)
/// The rows' labels (as measured by ink): their size, letter spacing, and how far below the row's
/// middle.
private let rowText: CGFloat = 15.1
private let rowTracking: CGFloat = -0.024 * 15.1
private let rowTextDrop: CGFloat = 0.75

// the menus' popover (as measured): the picker's panel, 249 tall, 17 off the middle with its
// arrow's tip 5.5 off it; rows from 54 under its top, 8 in; a plus 21 across and 3.6 thick, 30 from
// its right and 28 from its top; a row swiped 70 shows a red Delete 63 x 42, 4.5 in from its end
private let menusHeight: CGFloat = 249
private let menusOff: CGFloat = 17
private let menusTip: CGFloat = 5.5
private let menusRowsAt: CGFloat = 54
private let menusInset: CGFloat = 8
private let plusLength: CGFloat = 21
private let plusWeight: CGFloat = 3.6
private let plusInk = Color(argb: 0xFFC0_C0C0)
private let swipeOpen: CGFloat = 70
private let deleteWidth: CGFloat = 63
private let deleteHeight: CGFloat = 42
private let deleteIn: CGFloat = 4.5
private let deleteFill = Color(argb: 0xFFFE_4346)

/// The QuickMenu: its name in the middle of where it was opened, its six actions round it; an
/// action tapped runs and closes it, a tap anywhere else closes it, and a button held lists the
/// actions it can be set to.
struct QuickMenuView: View {
    let model: CanvasModel
    let screen: CGSize

    /// Where it shows, kept while it draws back in; how far its buttons have spread.
    @State private var at: CGPoint?
    @State private var shown: CGFloat = 0
    /// The button whose action is being chosen.
    @State private var choosing: Int?
    /// The menus' popover (the middle tapped): open, the menus added while it shows, the row blue.
    @State private var menusOpen = false
    @State private var fresh: [Int] = []
    @State private var marked: Int?

    private func closeMenus() {
        menusOpen = false
        fresh = []
    }

    /// A tap elsewhere closes the picker or the menus if open, else the menu.
    private func tapOutside() {
        if choosing != nil {
            choosing = nil
        } else if menusOpen {
            closeMenus()
        } else {
            model.quickMenuAt = nil
        }
    }

    var body: some View {
        ZStack(alignment: .topLeading) {
            if let at {
                Color.clear
                    .contentShape(Rectangle())
                    .onTapGesture(perform: tapOutside)
                ring(at)
                    .onAppear {
                        #if DEBUG
                            showForCapture()
                        #endif
                        withAnimation(Motion.quickMenuOpen) {
                            shown = 1
                        }
                    }
            }
        }
        .frame(width: screen.width, height: screen.height, alignment: .topLeading)
        .allowsHitTesting(model.quickMenuAt != nil)
        .onChange(of: model.quickMenuAt, initial: true) { _, target in
            if let target {
                if at != nil {
                    // back open while it drew in
                    withAnimation(Motion.quickMenuOpen) {
                        shown = 1
                    }
                }
                at = target
            } else if at != nil {
                choosing = nil
                closeMenus()
                withAnimation(Motion.quickMenuClose, completionCriteria: .logicallyComplete) {
                    shown = 0
                } completion: {
                    if model.quickMenuAt == nil {
                        at = nil
                    }
                }
            }
        }
    }

    #if DEBUG
        private func showForCapture() {
            switch ScreenCapture.quickShows {
            case "menus":
                menusOpen = true
                marked = QuickMenus.shared.active
            case "picker":
                choosing = 1
            default:
                break
            }
        }
    #endif

    private func ring(_ point: CGPoint) -> some View {
        // kept on the screen
        let centre = CGPoint(
            x: min(max(point.x, ringReachX + ringWidth / 2), screen.width - ringReachX - ringWidth / 2),
            y: min(max(point.y, ringReachY + ringHeight), screen.height - ringReachY - ringHeight)
        )
        let menus = QuickMenus.shared
        let slots = menus.menu?.slots ?? []
        return ZStack(alignment: .topLeading) {
            // tapped, it lists the menus
            Text(verbatim: menus.menu?.name ?? "")
                .font(.system(size: 16, weight: .semibold))
                .foregroundStyle(titleInk)
                .lineLimit(1)
                .frame(width: ringWidth, height: ringHeight)
                .background(Capsule().fill(titleFill))
                .contentShape(Capsule())
                .onTapGesture {
                    guard choosing == nil else { return }
                    if menusOpen {
                        closeMenus()
                    } else {
                        menusOpen = true
                        marked = menus.active
                    }
                }
                .opacity(shown)
                .offset(x: centre.x - ringWidth / 2, y: centre.y - ringHeight / 2)
            ForEach(Array(slots.indices), id: \.self) { i in
                if ringSlots.indices.contains(i) {
                    button(i, at: point, centre: centre)
                }
            }
            if let i = choosing, ringSlots.indices.contains(i) {
                let top = centre.y + ringSlots[i].y - ringHeight / 2
                ActionPicker(
                    current: slots.indices.contains(i) ? slots[i] : .none,
                    brush: model.quickSlotBrush(i),
                    sets: model.brushSets,
                    centreX: centre.x + ringSlots[i].x,
                    buttonTop: top,
                    buttonBottom: top + ringHeight,
                    screen: screen,
                    onBrush: {
                        model.setQuickSlotBrush(i, $0)
                        choosing = nil
                    },
                    onPick: {
                        menus.setSlot(i, $0)
                        choosing = nil
                    }
                )
            }
            if menusOpen {
                let left = centre.x - ringWidth / 2
                let top = centre.y - ringHeight / 2
                MenusPopover(
                    menu: CGRect(x: left, y: top, width: ringWidth, height: ringHeight),
                    screen: screen,
                    fresh: fresh,
                    marked: marked,
                    onAdd: {
                        // it shows, but the active one stays until a row is chosen
                        fresh.insert(menus.add(), at: 0)
                        marked = nil
                    },
                    onChoose: {
                        menus.choose($0)
                        marked = $0
                    },
                    onDelete: { i in
                        menus.delete(i)
                        fresh = fresh.filter { $0 != i }.map { $0 > i ? $0 - 1 : $0 }
                        marked = nil
                    }
                )
            }
        }
    }

    private func button(_ i: Int, at point: CGPoint, centre: CGPoint) -> some View {
        Text(verbatim: model.quickSlotLabel(i))
            .font(.system(size: 16, weight: .semibold))
            .foregroundStyle(buttonInk)
            .lineLimit(1)
            .multilineTextAlignment(.center)
            .frame(width: ringWidth, height: ringHeight)
            .background(Capsule().fill(buttonFill))
            .overlay(Capsule().strokeBorder(buttonEdge, lineWidth: 1))
            .contentShape(Capsule())
            .onTapGesture {
                if menusOpen {
                    closeMenus()
                } else {
                    model.quickMenuAt = nil
                    model.runQuickSlot(i, at: point)
                }
            }
            .onLongPressGesture(minimumDuration: 0.4) {
                if !menusOpen {
                    choosing = i
                }
            }
            .opacity(shown)
            .offset(
                x: centre.x + ringSlots[i].x * shown - ringWidth / 2,
                y: centre.y + ringSlots[i].y * shown - ringHeight / 2
            )
    }
}

/// A row's label in the picker and the menus' popover.
private func rowLabel(_ text: String, on: Bool) -> some View {
    Text(verbatim: text)
        .font(.system(size: rowText, weight: on ? .semibold : .regular))
        .tracking(rowTracking)
        .foregroundStyle(on ? Color.white : pickerRowInk)
        .lineLimit(1)
        .offset(y: rowTextDrop)
}

/// A page of a held button's picker.
private enum PickerPage {
    case actions, adjustments, brushSets, brushes
}

/// A row of the picker: what it says, whether it opens a list of its own, and what a tap does.
private struct PickerRow {
    let label: String
    let more: Bool
    let tap: () -> Void
}

/// A held QuickMenu button's picker, hung from the button by an arrow (above it when there is no
/// room below): every action by name, the button's own blue, scrolled to it. Adjustments opens
/// their own list; Select brush the brush sets, then a set's brushes. A row tapped sets the button.
private struct ActionPicker: View {
    let current: QuickAction
    let brush: BrushInfo?
    let sets: [BrushSetInfo]
    let centreX: CGFloat
    let buttonTop: CGFloat
    let buttonBottom: CGFloat
    let screen: CGSize
    let onBrush: (BrushInfo) -> Void
    let onPick: (QuickAction) -> Void

    @State private var page: PickerPage
    @State private var set = 0

    init(
        current: QuickAction, brush: BrushInfo?, sets: [BrushSetInfo], centreX: CGFloat, buttonTop: CGFloat,
        buttonBottom: CGFloat, screen: CGSize, onBrush: @escaping (BrushInfo) -> Void,
        onPick: @escaping (QuickAction) -> Void
    ) {
        self.current = current
        self.brush = brush
        self.sets = sets
        self.centreX = centreX
        self.buttonTop = buttonTop
        self.buttonBottom = buttonBottom
        self.screen = screen
        self.onBrush = onBrush
        self.onPick = onPick
        _page = State(initialValue: quickActionAdjustment(action: current) != nil ? .adjustments : .actions)
    }

    /// The page's title and where its way back goes.
    private var heading: (title: String, back: PickerPage?) {
        switch page {
        case .actions: ("Set Action", nil)
        case .adjustments: ("Adjustments", .actions)
        case .brushSets: ("Brush Set", .actions)
        case .brushes: ("Brushes", .brushSets)
        }
    }

    private var rows: [PickerRow] {
        switch page {
        case .actions:
            quickActionsListed().map { a in
                guard let a else {
                    return PickerRow(label: "Adjustments", more: true) { page = .adjustments }
                }
                let label = quickActionLabel(action: a)
                if a == .brushes {
                    return PickerRow(label: label, more: true) {
                        set = Int(brush?.set ?? 0)
                        page = .brushSets
                    }
                }
                return PickerRow(label: label, more: false) { onPick(a) }
            }
        case .adjustments:
            quickAdjustmentsListed().map { a in
                PickerRow(label: quickActionLabel(action: a), more: false) { onPick(a) }
            }
        case .brushSets:
            sets.enumerated().map { s, info in
                PickerRow(label: info.name, more: true) {
                    set = s
                    page = .brushes
                }
            }
        case .brushes:
            (sets.indices.contains(set) ? sets[set].brushes : []).map { b in
                PickerRow(label: b.name, more: false) { onBrush(b) }
            }
        }
    }

    /// The one row blue: in a list of its own, the button's, else the first.
    private var chosen: Int {
        switch page {
        case .actions:
            quickActionsListed().firstIndex(of: current) ?? -1
        case .adjustments:
            quickAdjustmentsListed().firstIndex(of: current) ?? 0
        case .brushSets:
            Int(brush?.set ?? 0)
        case .brushes:
            brush.flatMap { Int($0.set) == set ? Int($0.index) : nil } ?? 0
        }
    }

    /// The chosen row a little below the list's top; the first two stay where they are.
    private func show(_ reader: ScrollViewProxy) {
        let list = pickerHeight - pickerHeader
        let row = chosen
        DispatchQueue.main.async {
            if row >= 2 {
                reader.scrollTo(row, anchor: UnitPoint(x: 0.5, y: pickerChosenAt / (list - pickerRowHeight)))
            } else {
                reader.scrollTo(0, anchor: .top)
            }
        }
    }

    var body: some View {
        let below = buttonBottom + pickerArrow + pickerHeight <= screen.height - pickerMargin
        let left = min(max(centreX - pickerWidth / 2, pickerMargin), max(screen.width - pickerWidth - pickerMargin, pickerMargin))
        let top = below ? buttonBottom + pickerArrow : buttonTop - pickerArrow - pickerHeight
        let shape = RoundedRectangle(cornerRadius: pickerCorner, style: .circular)
        let head = heading
        let listed = rows
        let blue = chosen
        ZStack(alignment: .topLeading) {
            // the arrow, its point on the button
            Canvas { context, _ in
                let tip = below ? buttonBottom : buttonTop
                let base = below ? tip + pickerArrow + 1 : tip - pickerArrow - 1
                var path = Path()
                path.move(to: CGPoint(x: centreX, y: tip))
                path.addLine(to: CGPoint(x: centreX + pickerArrow, y: base))
                path.addLine(to: CGPoint(x: centreX - pickerArrow, y: base))
                path.closeSubpath()
                context.fill(path, with: .color(pickerFill))
            }
            .frame(width: screen.width, height: screen.height)
            .allowsHitTesting(false)
            VStack(spacing: 0) {
                // the title; in a list of its own a way back
                HStack(alignment: .top, spacing: 4) {
                    if head.back != nil {
                        PcIconView(PcIcons.chevronLeft, size: 24, color: pickerTitleInk)
                    }
                    Text(verbatim: head.title)
                        .font(.system(size: 19.3, weight: .bold))
                        .foregroundStyle(pickerTitleInk)
                        .lineLimit(1)
                    Spacer(minLength: 0)
                }
                .padding(.leading, head.back != nil ? 13 : 21)
                .padding(.top, 15)
                .frame(height: pickerHeader, alignment: .top)
                .contentShape(Rectangle())
                .onTapGesture {
                    if let back = head.back {
                        page = back
                    }
                }
                ScrollViewReader { reader in
                    ScrollView(.vertical, showsIndicators: false) {
                        VStack(spacing: pickerRowGap) {
                            ForEach(listed.indices, id: \.self) { k in
                                row(listed[k], on: k == blue)
                                    .id(k)
                            }
                        }
                        .padding(.horizontal, pickerInset)
                        .padding(.bottom, 6)
                    }
                    .onAppear { show(reader) }
                    .onChange(of: page) { show(reader) }
                }
            }
            .frame(width: pickerWidth, height: pickerHeight)
            .background(shape.fill(pickerFill))
            .clipShape(shape)
            .contentShape(shape)
            .onTapGesture {}
            .shadow(color: .black.opacity(0.35), radius: 12)
            .offset(x: left, y: top)
        }
    }

    private func row(_ row: PickerRow, on: Bool) -> some View {
        ZStack {
            rowLabel(row.label, on: on)
                .frame(maxWidth: .infinity, alignment: .leading)
            if row.more {
                PcIconView(PcIcons.chevronRight, size: 18, color: on ? Color.white : pickerRowInk)
                    .frame(maxWidth: .infinity, alignment: .trailing)
            }
        }
        .padding(.horizontal, 16)
        .frame(height: pickerRowHeight)
        .background(Capsule().fill(on ? Pc.accent : pickerRowFill))
        .contentShape(Capsule())
        .onTapGesture(perform: row.tap)
    }
}

/// The QuickMenu's menus, hung from the middle (below it where it fits, else above, else beside): a
/// plus adds one and shows it, a row tapped makes it the active one, a row swiped left shows Delete
/// (swiped most of the way, it goes at once). Menus added while it shows are listed first.
private struct MenusPopover: View {
    /// Where the middle is.
    let menu: CGRect
    let screen: CGSize
    let fresh: [Int]
    let marked: Int?
    let onAdd: () -> Void
    let onChoose: (Int) -> Void
    let onDelete: (Int) -> Void

    /// The row swiped and how far.
    @State private var swiped: Int?
    @State private var offset: CGFloat = 0

    /// Where the panel goes, and which of below, above, right, left that is.
    private var place: (at: CGPoint, side: Int) {
        let places = [
            CGPoint(x: menu.midX - pickerWidth / 2, y: menu.maxY + menusOff),
            CGPoint(x: menu.midX - pickerWidth / 2, y: menu.minY - menusOff - menusHeight),
            CGPoint(x: menu.maxX + menusOff, y: menu.midY - menusHeight / 2),
            CGPoint(x: menu.minX - menusOff - pickerWidth, y: menu.midY - menusHeight / 2),
        ]
        let side = places.firstIndex {
            $0.x >= pickerMargin && $0.y >= pickerMargin && $0.x + pickerWidth <= screen.width - pickerMargin
                && $0.y + menusHeight <= screen.height - pickerMargin
        } ?? 0
        return (places[side], side)
    }

    /// The arrow, its point just off the middle.
    private func arrow(_ at: CGPoint, _ side: Int) -> Path {
        let a = pickerArrow
        var path = Path()
        let points: [CGPoint] = switch side {
        case 0:
            [
                CGPoint(x: menu.midX, y: menu.maxY + menusTip),
                CGPoint(x: menu.midX + a, y: at.y + 1), CGPoint(x: menu.midX - a, y: at.y + 1),
            ]
        case 1:
            [
                CGPoint(x: menu.midX, y: menu.minY - menusTip),
                CGPoint(x: menu.midX - a, y: at.y + menusHeight - 1),
                CGPoint(x: menu.midX + a, y: at.y + menusHeight - 1),
            ]
        case 2:
            [
                CGPoint(x: menu.maxX + menusTip, y: menu.midY),
                CGPoint(x: at.x + 1, y: menu.midY - a), CGPoint(x: at.x + 1, y: menu.midY + a),
            ]
        default:
            [
                CGPoint(x: menu.minX - menusTip, y: menu.midY),
                CGPoint(x: at.x + pickerWidth - 1, y: menu.midY + a),
                CGPoint(x: at.x + pickerWidth - 1, y: menu.midY - a),
            ]
        }
        path.addLines(points)
        path.closeSubpath()
        return path
    }

    var body: some View {
        let placed = place
        let menus = QuickMenus.shared.sets
        let shape = RoundedRectangle(cornerRadius: pickerCorner, style: .circular)
        // the menus added this time first, newest first, then the rest as kept
        let order = fresh.filter { menus.indices.contains($0) } + menus.indices.filter { !fresh.contains($0) }
        ZStack(alignment: .topLeading) {
            Canvas { context, _ in
                context.fill(arrow(placed.at, placed.side), with: .color(pickerFill))
            }
            .frame(width: screen.width, height: screen.height)
            .allowsHitTesting(false)
            ZStack(alignment: .topLeading) {
                Text(verbatim: "QuickMenu")
                    .font(.system(size: 19.3, weight: .bold))
                    .foregroundStyle(pickerTitleInk)
                    .lineLimit(1)
                    .padding(.leading, 21)
                    .padding(.top, 16)
                // the plus, its bars' ends round
                ZStack {
                    Capsule().fill(plusInk).frame(width: plusLength, height: plusWeight)
                    Capsule().fill(plusInk).frame(width: plusWeight, height: plusLength)
                }
                .frame(width: 44, height: 44)
                .contentShape(Rectangle())
                .onTapGesture(perform: onAdd)
                .offset(x: pickerWidth - 30 - 22, y: 28 - 22)
                ScrollView(.vertical, showsIndicators: false) {
                    VStack(spacing: pickerRowGap) {
                        ForEach(order, id: \.self) { i in
                            row(i, name: menus[i].name, swipes: menus.count > 1)
                        }
                    }
                    .padding(.horizontal, menusInset)
                    .padding(.bottom, 6)
                }
                .padding(.top, menusRowsAt)
                .padding(.bottom, 9.5)
            }
            .frame(width: pickerWidth, height: menusHeight, alignment: .topLeading)
            .background(shape.fill(pickerFill))
            .clipShape(shape)
            .contentShape(shape)
            .onTapGesture {}
            .shadow(color: .black.opacity(0.35), radius: 12)
            .offset(x: placed.at.x, y: placed.at.y)
        }
    }

    private func row(_ i: Int, name: String, swipes: Bool) -> some View {
        let on = marked == i
        let moved = swiped == i ? offset : 0
        let width = pickerWidth - menusInset * 2
        return ZStack(alignment: .trailing) {
            // the Delete, shown as far as the row has moved off it
            Text(verbatim: "Delete")
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(Color.white)
                .lineLimit(1)
                .frame(width: deleteWidth, height: deleteHeight)
                .background(Capsule().fill(deleteFill))
                .mask(alignment: .trailing) {
                    Rectangle().frame(width: min(max(-moved - deleteIn - 2.5, 0), deleteWidth))
                }
                .contentShape(Rectangle())
                .onTapGesture {
                    if moved <= -swipeOpen {
                        swiped = nil
                        offset = 0
                        onDelete(i)
                    }
                }
                .padding(.trailing, deleteIn)
            rowLabel(name, on: on)
                .padding(.horizontal, 16)
                .frame(width: width, height: pickerRowHeight, alignment: .leading)
                .background(Capsule().fill(on ? Pc.accent : pickerRowFill))
                .contentShape(Capsule())
                .offset(x: moved)
                // a tap closes a row left open, else chooses the menu
                .onTapGesture {
                    if swiped != nil {
                        withAnimation(Motion.swipeSettle) {
                            swiped = nil
                            offset = 0
                        }
                    } else {
                        onChoose(i)
                    }
                }
                .simultaneousGesture(swipe(i, width: width, enabled: swipes))
        }
        .frame(width: width, height: pickerRowHeight)
    }

    private func swipe(_ i: Int, width: CGFloat, enabled: Bool) -> some Gesture {
        DragGesture(minimumDistance: 14)
            .onChanged { drag in
                guard enabled, abs(drag.translation.width) > abs(drag.translation.height) || swiped == i else { return }
                let from = swiped == i && offset <= -swipeOpen ? -swipeOpen : 0
                swiped = i
                offset = min(max(from + drag.translation.width, -width), 0)
            }
            .onEnded { _ in
                guard swiped == i else { return }
                // past most of the row it goes; past half the Delete it stays open
                if offset < -width * 0.6 {
                    swiped = nil
                    offset = 0
                    onDelete(i)
                } else {
                    withAnimation(Motion.swipeSettle) {
                        if offset < -swipeOpen / 2 {
                            offset = -swipeOpen
                        } else {
                            swiped = nil
                            offset = 0
                        }
                    }
                }
            }
    }
}

private func panelFill() -> Color {
    Color(argb: Look.shared.light ? 0xFFE2_E2E2 : 0xFF48_4848)
}

private func panelTitleInk() -> Color {
    Color(argb: Look.shared.light ? 0xFF39_3939 : 0xFFC5_C5C5)
}

private func panelButtonInk() -> Color {
    Color(argb: Look.shared.light ? 0xFF4A_4A4A : 0xFFA8_A8A8)
}

private func panelCloseFill() -> Color {
    Color(argb: Look.shared.light ? 0xFFCF_CFCF : 0xFF2A_2A2A)
}

/// The Copy & Paste panel (three fingers swiped down): Cut, Copy, Copy All, Duplicate, Cut & Paste
/// and Paste, a little below the middle of the screen; a tap off it or its cross closes it.
struct CopyPastePanel: View {
    let model: CanvasModel
    let screen: CGSize

    private func act(_ f: @escaping () -> Void) -> () -> Void {
        {
            model.copyPasteOpen = false
            f()
        }
    }

    private func button(_ icon: PcIcon, _ label: String, enabled: Bool = true, action: @escaping () -> Void) -> some View {
        let ink = panelButtonInk().opacity(enabled ? 1 : 0.4)
        // the icon (a 50 frame) 28 across, centred 17 down; the name from 34
        return ZStack(alignment: .top) {
            PcIconView(icon, size: 28, color: ink)
                .padding(.top, 3)
            Text(verbatim: label)
                .font(.system(size: 13))
                .foregroundStyle(ink)
                .lineLimit(1)
                .fixedSize()
                .padding(.top, 33)
        }
        .frame(width: 82.7, height: 50, alignment: .top)
        .contentShape(Rectangle())
        .onTapGesture {
            if enabled {
                action()
            }
        }
    }

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 30, style: .circular)
        ZStack(alignment: .topLeading) {
            Color.clear
                .contentShape(Rectangle())
                .onTapGesture { model.copyPasteOpen = false }
            ZStack(alignment: .topLeading) {
                Capsule()
                    .fill(Color(argb: 0xFF7C_7C7C))
                    .frame(width: 40, height: 4)
                    .padding(.top, 5)
                    .frame(maxWidth: .infinity)
                Text(verbatim: "Copy & Paste")
                    .font(.system(size: 22, weight: .bold))
                    .foregroundStyle(panelTitleInk())
                    .lineLimit(1)
                    .fixedSize()
                    .offset(x: 18, y: 12)
                PcIconView(PcIcons.cross, size: 18, color: panelButtonInk())
                    .frame(width: 40, height: 40)
                    .background(Circle().fill(panelCloseFill()))
                    .contentShape(Circle())
                    .onTapGesture { model.copyPasteOpen = false }
                    .offset(x: 453, y: 6)
                HStack(spacing: 0) {
                    button(PcIcons.copyCut, "Cut", action: act(model.cutLayer))
                    button(PcIcons.copyOne, "Copy", action: act(model.copyLayer))
                    button(PcIcons.copyAll, "Copy All", action: act(model.copyCanvas))
                    button(PcIcons.copyDuplicate, "Duplicate", action: act(model.duplicateCurrent))
                    button(PcIcons.cutPaste, "Cut & Paste", action: act(model.cutAndPaste))
                    button(PcIcons.pasteDown, "Paste", enabled: model.canPaste, action: act({ model.paste() }))
                }
                .offset(y: 48)
            }
            .frame(width: 496, height: 106, alignment: .topLeading)
            .background(shape.fill(panelFill()))
            .clipShape(shape)
            .contentShape(shape)
            .onTapGesture {}
            .offset(x: (screen.width - 496) / 2, y: screen.height * (617.0 / 1133.0))
        }
    }
}
