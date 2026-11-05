import SwiftUI
import UIKit

// as measured: a thumbnail's shadow dropped below it, and the rim round it
private let thumbShadowDrop: CGFloat = 5
private var thumbShadow: Color { Look.shared.light ? Color(argb: 0x1D00_0000) : Color(argb: 0x4500_0000) }
private var thumbRim: Color { Look.shared.light ? Color(argb: 0xFFBE_BEBE) : Color(argb: 0xFF42_4242) }

/// A gallery cell's sizes: its pitch both ways (the screen's width over the columns, three upright
/// and four on its side), the content's width (40 less) and the thumbnail box's height (82 less).
private struct CellSize {
    let pitch: CGFloat
    let width: CGFloat
    let thumb: CGFloat

    init(screenWidth: CGFloat, columns: Int) {
        pitch = screenWidth / CGFloat(columns)
        width = pitch - 40
        thumb = pitch - 82
    }
}

/// The gallery: its title and buttons, then the artworks kept on the iPad (the core's store, in
/// the app's own Application Support), each opened by a tap; + makes a new canvas the size of the
/// screen.
struct GalleryView: View {
    let onOpen: (OpenArtwork) -> Void

    @Environment(\.displayScale) private var displayScale
    @State private var artworks: [GalleryArtwork] = []
    @State private var failure: String?
    /// Goes up when artworks are saved, so thumbnails read again.
    @State private var thumbnailsVersion = 0

    init(onOpen: @escaping (OpenArtwork) -> Void) {
        self.onOpen = onOpen
    }

    var body: some View {
        GeometryReader { screen in
            let columns = screen.size.width > screen.size.height ? 4 : 3
            let cell = CellSize(screenWidth: screen.size.width, columns: columns)
            VStack(spacing: 0) {
                GalleryHeader { newCanvas(points: screen.size) }
                ScrollView {
                    LazyVGrid(columns: Array(repeating: GridItem(.fixed(cell.pitch), spacing: 0), count: columns), spacing: 0) {
                        ForEach(artworks, id: \.dir) { artwork in
                            ArtworkCell(artwork: artwork, cell: cell, version: thumbnailsVersion)
                                .contentShape(Rectangle())
                                .onTapGesture { open(artwork) }
                        }
                    }
                    .padding(.bottom, 24)
                }
                .scrollIndicators(.hidden)
            }
        }
        .background(Pc.galleryBackground.ignoresSafeArea())
        .onAppear(perform: reload)
        // an artwork just left is saved while the gallery shows: its new thumbnail read then
        .onReceive(NotificationCenter.default.publisher(for: Gallery.keptNotification)) { note in
            reload()
            thumbnailsVersion += 1
            if let reason = note.object as? String {
                failure = saveFailureSaid(reason: reason)
            }
        }
        .alert("Can't open the artwork", isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } })) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(verbatim: failure ?? "")
        }
    }

    /// The gallery's artworks, those in stacks among them, newest first as the store orders them.
    private func reload() {
        artworks = Gallery.store.list(folder: nil).flatMap { entry -> [GalleryArtwork] in
            switch entry {
            case let .artwork(artwork):
                return [artwork]
            case let .stack(_, _, artworks):
                return artworks
            }
        }
    }

    /// A new artwork the size of the screen (`points` across, in pixels), in a new folder.
    private func newCanvas(points: CGSize) {
        let width = UInt32(max(1, (points.width * displayScale).rounded()))
        let height = UInt32(max(1, (points.height * displayScale).rounded()))
        let session = CanvasSession(width: width, height: height)
        onOpen(OpenArtwork(session: session, dir: Gallery.store.newFolder(folder: nil)))
    }

    private func open(_ artwork: GalleryArtwork) {
        do {
            onOpen(OpenArtwork(session: try CanvasSession.open(dir: artwork.dir), dir: artwork.dir))
        } catch {
            failure = "\(error)"
        }
    }
}

/// The gallery's title row: the app's name, then Select, Import and Photo in a capsule and the
/// round + that makes a new canvas.
private struct GalleryHeader: View {
    let onNew: () -> Void

    var body: some View {
        ZStack(alignment: .topLeading) {
            Text(verbatim: "ProCraft")
                .font(.system(size: 36, weight: .bold))
                .foregroundStyle(Pc.galleryTitle)
                .padding(.leading, 32)
                .padding(.top, 11)
            HStack(spacing: 0) {
                Spacer()
                HStack(spacing: 0) {
                    ForEach([("Select", CGFloat(69)), ("Import", 71), ("Photo", 66)], id: \.0) { button in
                        Text(verbatim: button.0)
                            .font(.system(size: 16))
                            .foregroundStyle(Pc.galleryText)
                            .frame(width: button.1, height: 45)
                    }
                }
                .background(Capsule().fill(Pc.capsule))
                .overlay(Capsule().strokeBorder(Pc.capsuleEdge, lineWidth: 1))
                ZStack {
                    Circle().fill(Pc.capsule).frame(width: 42, height: 42)
                    Circle().strokeBorder(Pc.capsuleEdge, lineWidth: 1).frame(width: 42, height: 42)
                    PcIconView(PcIcons.add, size: 40, color: Pc.galleryText)
                }
                .frame(width: 44, height: 45)
                .contentShape(Rectangle())
                .onTapGesture(perform: onNew)
                .padding(.leading, 12)
            }
            .padding(.top, 10)
            .padding(.trailing, 20)
        }
        .frame(height: 55, alignment: .top)
    }
}

/// An artwork's cell: its thumbnail, as large as fits in the cell's box with a soft shadow under
/// it, then its name and size.
private struct ArtworkCell: View {
    let artwork: GalleryArtwork
    let cell: CellSize
    let version: Int
    @State private var picture: UIImage?

    var body: some View {
        let ratio = artwork.width > 0 && artwork.height > 0 ? CGFloat(artwork.width) / CGFloat(artwork.height) : 0.75
        let tall = ratio <= cell.width / cell.thumb
        let size = tall ? CGSize(width: cell.thumb * ratio, height: cell.thumb) : CGSize(width: cell.width, height: cell.width / ratio)
        let corner = RoundedRectangle(cornerRadius: 8, style: .circular)
        VStack(spacing: 0) {
            ZStack(alignment: .bottom) {
                ZStack {
                    // a picture with its background hidden shows the gallery through it (white while
                    // it loads)
                    corner.fill(picture == nil ? Color.white : Pc.galleryBackground)
                    if let picture {
                        Image(uiImage: picture)
                            .resizable()
                            .interpolation(picture.size.width * picture.scale <= size.width * 3 ? .none : .medium)
                    }
                }
                .frame(width: size.width, height: size.height)
                .clipShape(corner)
                .overlay(corner.strokeBorder(thumbRim, lineWidth: 1))
                .background(
                    corner.fill(thumbShadow)
                        .blur(radius: 2)
                        .offset(y: thumbShadowDrop)
                )
            }
            .frame(width: cell.width, height: cell.thumb, alignment: .bottom)
            .padding(.top, 39)
            Text(verbatim: artwork.name)
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(Pc.galleryText)
                .lineLimit(1)
                .truncationMode(.tail)
                .padding(.top, 12)
            Text(verbatim: "\(artwork.width) × \(artwork.height)px")
                .font(.system(size: 10))
                .foregroundStyle(Pc.gallerySize)
        }
        .frame(width: cell.pitch, height: cell.pitch, alignment: .top)
        .task(id: "\(artwork.modifiedMs)-\(version)") {
            let path = (artwork.dir as NSString).appendingPathComponent("thumbnail.png")
            picture = await Task.detached(priority: .userInitiated) { UIImage(contentsOfFile: path) }.value
        }
    }
}

/// Where the gallery's artworks are kept.
enum Gallery {
    /// An artwork left has been saved; its object is why it couldn't be, if it couldn't.
    static let keptNotification = Notification.Name("ProCraft.artworkKept")

    static func artworkKept(failure: String?) {
        NotificationCenter.default.post(name: keptNotification, object: failure)
    }

    static let store: ArtworkStore = {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSTemporaryDirectory())
        return ArtworkStore.open(root: support.appendingPathComponent("gallery").path)
    }()
}
