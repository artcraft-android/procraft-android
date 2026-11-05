import SwiftUI
import UIKit

private let newCanvasWidth: CGFloat = 347
private let formatsWidth: CGFloat = 260
private let formatsTitleHeight: CGFloat = 55
private let formatsRow: CGFloat = 40
private var formatsHeight: CGFloat {
    let n = CGFloat(GalleryModel.shareFormats.count)
    return formatsTitleHeight + formatsRow * n + (n - 1) + 10
}

/// How far a preset row slides, and what shows behind it.
private let swipeReveal: CGFloat = 159
private let swipeBehind = Color(argb: 0xFFD6_D6D6)
private let editFill = Color(argb: 0xFFC6_C6C6)
private let editInk = Color(argb: 0xFF57_5757)
private let deleteFill = Color(argb: 0xFFFE_604D)
private let renameFrameWidth: CGFloat = 281
private let renameFrameHeight: CGFloat = 280
private let renameTop: CGFloat = 20
private let renameArtMargin: CGFloat = 12
private let renameFieldGap: CGFloat = 20
private let renameFieldWidth: CGFloat = 262
private let renameFieldHeight: CGFloat = 38
private let renameFieldFill = Color(argb: 0x14FF_FFFF)

/// The new-canvas list hanging from the + button, 2 from the screen's edge: the screen's size
/// first, then the presets, each with its colour profile and size; a preset row swiped left shows
/// Edit and Delete.
struct NewCanvasPanel: View {
    let model: GalleryModel
    let screen: CGSize
    let scale: CGFloat
    let onDone: () -> Void

    /// The row swiped open, if one is.
    @State private var opened: Int?

    private var header: some View {
        ZStack(alignment: .topTrailing) {
            Text(verbatim: "New canvas")
                .font(.system(size: 26, weight: .bold))
                .foregroundStyle(Pc.lightText)
                .padding(.leading, 23)
                .padding(.top, 21)
                .padding(.bottom, 20)
                .frame(maxWidth: .infinity, alignment: .leading)
            // the button that makes a custom canvas
            Canvas { context, size in
                let shape = CGRect(x: 0, y: 0, width: size.width, height: size.height)
                context.fill(Path(roundedRect: shape, cornerRadius: 4, style: .circular), with: .color(Pc.lightText))
                var plus = Path()
                plus.move(to: CGPoint(x: 8 - 3.6, y: size.height / 2))
                plus.addLine(to: CGPoint(x: 8 + 3.6, y: size.height / 2))
                plus.move(to: CGPoint(x: 8, y: size.height / 2 - 3.6))
                plus.addLine(to: CGPoint(x: 8, y: size.height / 2 + 3.6))
                context.stroke(plus, with: .color(Pc.lightPanel), style: StrokeStyle(lineWidth: 1.6, lineCap: .round))
            }
            .frame(width: 27, height: 16)
            .frame(width: 44, height: 34)
            .contentShape(Rectangle())
            .onTapGesture { model.openCustom() }
            .padding(.top, 18)
            .padding(.trailing, 13)
        }
    }

    var body: some View {
        // the screen as it is turned now
        let width = UInt32(max((screen.width * scale).rounded(), 1))
        let height = UInt32(max((screen.height * scale).rounded(), 1))
        let left = screen.width - 2 - newCanvasWidth
        Popover(width: newCanvasWidth, arrowX: screen.width - 49 - left, fill: Pc.lightPanel, corner: 32) {
            VStack(spacing: 0) {
                header
                PresetRow(name: "Screen Size", label: "\(width) × \(height)px", profile: "Display P3") {
                    onDone()
                    model.newCanvas(width: width, height: height)
                }
                .frame(height: 38)
                .background(Capsule().fill(Pc.lightRowStrong))
                .padding(.horizontal, 9)
                // the presets scroll past 376 tall
                ScrollView(.vertical, showsIndicators: false) {
                    VStack(spacing: 0) {
                        ForEach(model.presets.indices, id: \.self) { i in
                            if i > 0 {
                                Rectangle()
                                    .fill(Pc.lightSeparator)
                                    .frame(height: 1)
                                    .padding(.leading, 16)
                            }
                            SwipeRow(
                                preset: model.presets[i], open: opened == i,
                                onOpen: { opened = $0 ? i : nil },
                                onEdit: {
                                    opened = nil
                                    model.openCustom(i)
                                },
                                onDelete: {
                                    opened = nil
                                    model.deletePreset(i)
                                },
                                onPick: {
                                    if opened != nil {
                                        opened = nil
                                    } else {
                                        let p = model.presets[i]
                                        onDone()
                                        model.newCanvas(p)
                                    }
                                }
                            )
                            .frame(height: i > 0 ? 44 : 45)
                        }
                    }
                }
                .frame(maxHeight: min(376, CGFloat(model.presets.count) * 45))
                .background(Pc.lightRow)
                .clipShape(RoundedRectangle(cornerRadius: 24, style: .circular))
                .padding(.horizontal, 9)
                .padding(.top, 9.5)
                .padding(.bottom, 30)
            }
        }
        .contentShape(Rectangle())
        .onTapGesture {}
        .offset(x: left, y: 56.5)
    }
}

/// A row's name, its colour profile and its size.
private struct PresetRow: View {
    let name: String
    let label: String
    let profile: String
    let action: () -> Void

    private func chip(_ text: String) -> some View {
        Text(verbatim: text)
            .font(.system(size: 13))
            .foregroundStyle(Pc.lightText)
            .lineLimit(1)
            .padding(.horizontal, 8)
            .frame(height: 18)
            .background(Capsule().fill(Pc.lightChip))
    }

    var body: some View {
        HStack(spacing: 0) {
            Text(verbatim: name)
                .font(.system(size: 17))
                .foregroundStyle(Pc.lightText)
                .lineLimit(1)
                .truncationMode(.tail)
            Spacer(minLength: 4)
            chip(profileChip(profile: profile))
            chip(label)
                .padding(.leading, 4)
        }
        .padding(.leading, 17)
        .padding(.trailing, 16)
        .contentShape(Rectangle())
        .onTapGesture(perform: action)
    }
}

/// A preset row that slides left under a finger to show Edit and Delete; a tap picks it (or closes
/// it, open).
private struct SwipeRow: View {
    let preset: CanvasPreset
    let open: Bool
    let onOpen: (Bool) -> Void
    let onEdit: () -> Void
    let onDelete: () -> Void
    let onPick: () -> Void

    @State private var moved: CGFloat?

    private func button(_ label: String, fill: Color, ink: Color, action: @escaping () -> Void) -> some View {
        Text(verbatim: label)
            .font(.system(size: 15, weight: .semibold))
            .foregroundStyle(ink)
            .frame(width: 69, height: 33)
            .background(RoundedRectangle(cornerRadius: 7, style: .circular).fill(fill))
            .contentShape(Rectangle())
            .onTapGesture(perform: action)
    }

    var body: some View {
        let x = moved ?? (open ? -swipeReveal : 0)
        ZStack(alignment: .trailing) {
            swipeBehind
            HStack(spacing: 3) {
                button("Edit", fill: editFill, ink: editInk, action: onEdit)
                button("Delete", fill: deleteFill, ink: .white, action: onDelete)
            }
            .padding(.trailing, 13)
            PresetRow(name: preset.name, label: presetLabel(preset: preset), profile: preset.profile, action: onPick)
                .frame(maxHeight: .infinity)
                .background(Pc.lightRow)
                .offset(x: x)
                .simultaneousGesture(
                    DragGesture(minimumDistance: 14)
                        .onChanged { drag in
                            guard moved != nil || abs(drag.translation.width) > abs(drag.translation.height) else { return }
                            let from: CGFloat = open ? -swipeReveal : 0
                            moved = min(max(from + drag.translation.width, -swipeReveal), 0)
                        }
                        .onEnded { _ in
                            guard let end = moved else { return }
                            withAnimation(Motion.swipeSettle) {
                                moved = nil
                                onOpen(end < -swipeReveal / 2)
                            }
                        }
                )
        }
        .clipped()
    }
}

/// Share's formats: beside a swiped artwork's Share, or under Select's. A tap on one shares in it.
struct ImageFormats: View {
    let share: GalleryShare
    let screen: CGSize
    let onPick: (String) -> Void

    /// Where they go: their top left, the arrow's side and place.
    private var place: (left: CGFloat, top: CGFloat, side: ArrowSide, arrowAt: CGFloat) {
        guard let from = share.from else {
            // Select's Share button's centre: left of Delete and More
            let centre = screen.width - 20 - 44 - 13 - 61 - 70 - 65 / 2
            let left = min(max(centre - formatsWidth / 2, 4), max(screen.width - 4 - formatsWidth, 4))
            return (left, 69, .none, 0)
        }
        let gap: CGFloat = 2
        let top = min(max(from.midY - formatsHeight / 2, 2), max(screen.height - 2 - formatsHeight, 2))
        let arrowAt = min(max(from.midY - top, Pc.popoverCorner + Pc.arrowHeight), formatsHeight - Pc.popoverCorner - Pc.arrowHeight)
        let total = formatsWidth + Pc.arrowHeight
        // on the side of the pill with more room, the other if that side is too narrow
        let roomRight = screen.width - 2 - from.maxX - gap
        let roomLeft = from.minX - gap - 2
        let right = roomRight >= roomLeft ? roomRight >= total || roomLeft < total : roomLeft < total && roomRight >= total
        if right {
            return (from.maxX + gap, top, .left, arrowAt)
        }
        return (max(from.minX - gap - total, 2), top, .right, arrowAt)
    }

    var body: some View {
        let at = place
        let formats = GalleryModel.shareFormats
        let shape = RoundedRectangle(cornerRadius: 18, style: .circular)
        Popover(width: formatsWidth, arrowX: at.arrowAt, fill: Pc.galleryPopover, side: at.side) {
            VStack(alignment: .leading, spacing: 0) {
                Text(verbatim: "Image Format")
                    .font(.system(size: 24, weight: .bold))
                    .foregroundStyle(Pc.panelTitle)
                    .padding(.leading, 22)
                    .frame(height: formatsTitleHeight)
                VStack(spacing: 0) {
                    ForEach(formats.indices, id: \.self) { i in
                        if i > 0 {
                            Rectangle().fill(Pc.actionsSeparator).frame(height: 1)
                        }
                        Text(verbatim: formats[i])
                            .font(.system(size: 17))
                            .foregroundStyle(Pc.panelTitle)
                            .padding(.leading, 16)
                            .frame(maxWidth: .infinity, minHeight: formatsRow, maxHeight: formatsRow, alignment: .leading)
                            .contentShape(Rectangle())
                            .onTapGesture { onPick(formats[i]) }
                    }
                }
                .background(shape.fill(Pc.actionsGroup))
                .clipShape(shape)
                .padding(.horizontal, 10)
                .padding(.bottom, 10)
            }
        }
        .contentShape(Rectangle())
        .onTapGesture {}
        .offset(x: at.left, y: at.top)
    }
}

/// More options for the picked artworks: Stack (two or more) and Duplicate, under More.
struct MoreOptions: View {
    let screen: CGSize
    let count: Int
    let canStack: Bool
    let onStack: () -> Void
    let onDuplicate: () -> Void

    private func row(_ label: String, enabled: Bool, action: @escaping () -> Void) -> some View {
        Text(verbatim: label)
            .font(.system(size: 17))
            .foregroundStyle(enabled ? Pc.panelTitle : Pc.actionsDisabled)
            .padding(.leading, 16)
            .frame(maxWidth: .infinity, minHeight: 44, maxHeight: 44, alignment: .leading)
            .contentShape(Rectangle())
            .onTapGesture {
                if enabled {
                    action()
                }
            }
    }

    var body: some View {
        // under More, kept 4 from the screen's edge
        let centre = screen.width - 20 - 44 - 13 - 61 / 2
        let left = min(centre - 125, screen.width - 4 - 250)
        let shape = RoundedRectangle(cornerRadius: 18, style: .circular)
        Popover(width: 250, arrowX: centre - left, fill: Pc.galleryPopover) {
            VStack(alignment: .leading, spacing: 0) {
                Text(verbatim: "More Options")
                    .font(.system(size: 24, weight: .bold))
                    .foregroundStyle(Pc.panelTitle)
                    .padding(.leading, 24)
                    .padding(.vertical, 14)
                VStack(spacing: 0) {
                    row("Stack", enabled: canStack && count >= 2, action: onStack)
                    Rectangle().fill(Pc.actionsSeparator).frame(height: 1)
                    row("Duplicate", enabled: count > 0, action: onDuplicate)
                }
                .background(shape.fill(Pc.actionsGroup))
                .clipShape(shape)
                .padding(.horizontal, 10)
                .padding(.bottom, 12)
            }
        }
        .contentShape(Rectangle())
        .onTapGesture {}
        .offset(x: left, y: 60)
    }
}

/// Renaming an artwork: it rises from its cell (`from`, on the screen) to the top middle, enlarged,
/// with its name in a field under it and the keyboard up. Done, or tapped away, it calls `onDone`
/// with the name.
struct RenameArtwork: View {
    let renaming: Renaming
    let from: CGRect?
    let screen: CGSize
    let onDone: (String) -> Void

    @State private var text: String
    @State private var picture: UIImage?
    @State private var risen = false
    @State private var finished = false
    @FocusState private var focused: Bool

    init(renaming: Renaming, from: CGRect?, screen: CGSize, onDone: @escaping (String) -> Void) {
        self.renaming = renaming
        self.from = from
        self.screen = screen
        self.onDone = onDone
        _text = State(initialValue: renaming.name)
    }

    private func finish() {
        guard !finished else { return }
        finished = true
        let name = text.trimmingCharacters(in: .whitespaces)
        onDone(name.isEmpty ? renaming.name : name)
    }

    /// The art fitted in its frame, centred across the screen.
    private var target: CGRect {
        let a = renaming.shown
        let ratio = a.width > 0 && a.height > 0 ? CGFloat(a.width) / CGFloat(a.height) : 0.75
        let fw = renameFrameWidth - renameArtMargin * 2
        let fh = renameFrameHeight - renameArtMargin * 2
        let wide = ratio > fw / fh
        let w = wide ? fw : fh * ratio
        let h = wide ? fw / ratio : fh
        return CGRect(x: (screen.width - w) / 2, y: renameTop + (renameFrameHeight - h) / 2, width: w, height: h)
    }

    var body: some View {
        let to = target
        let at = risen ? to : from ?? to
        let corner = RoundedRectangle(cornerRadius: 8, style: .circular)
        ZStack(alignment: .topLeading) {
            Color.clear
                .contentShape(Rectangle())
                .onTapGesture(perform: finish)
            ZStack {
                corner.fill(Color.white)
                if let picture {
                    Image(uiImage: picture)
                        .resizable()
                }
            }
            .clipShape(corner)
            .frame(width: at.width, height: at.height)
            .offset(x: at.minX, y: at.minY)
            .allowsHitTesting(false)
            TextField("", text: $text)
                .font(.system(size: 17))
                .foregroundStyle(Pc.text)
                .multilineTextAlignment(.center)
                .tint(Pc.accent)
                .submitLabel(.done)
                .focused($focused)
                .onSubmit(finish)
                .padding(.horizontal, 12)
                .frame(width: renameFieldWidth, height: renameFieldHeight)
                .background(RoundedRectangle(cornerRadius: 10, style: .circular).fill(renameFieldFill))
                .opacity(risen ? 1 : 0)
                .offset(
                    x: (screen.width - renameFieldWidth) / 2,
                    y: renameTop + renameFrameHeight + renameFieldGap
                )
        }
        .task {
            let path = (renaming.shown.dir as NSString).appendingPathComponent("thumbnail.png")
            picture = await Task.detached(priority: .userInitiated) { UIImage(contentsOfFile: path) }.value
        }
        .onAppear {
            withAnimation(.easeInOut(duration: 0.3)) {
                risen = true
            }
            focused = true
        }
    }
}
