import SwiftUI
import UIKit

/// The artworks kept on the iPad (the core's store, in the app's own Application Support), each
/// opened by a tap, and a new canvas the size of the screen.
struct GalleryView: View {
    let onOpen: (OpenArtwork) -> Void

    @Environment(\.displayScale) private var displayScale
    @State private var artworks: [GalleryArtwork] = []
    @State private var failure: String?

    init(onOpen: @escaping (OpenArtwork) -> Void) {
        self.onOpen = onOpen
    }

    var body: some View {
        GeometryReader { screen in
            NavigationStack {
                ScrollView {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 200), spacing: 24)], spacing: 32) {
                        ForEach(artworks, id: \.dir) { artwork in
                            Button {
                                open(artwork)
                            } label: {
                                Thumbnail(artwork: artwork)
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(24)
                }
                .navigationTitle("Gallery")
                .toolbar {
                    ToolbarItem(placement: .primaryAction) {
                        Button("New canvas") { newCanvas(points: screen.size) }
                    }
                }
            }
        }
        .onAppear(perform: reload)
        .alert("Can't open the artwork", isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } })) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(failure ?? "")
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

/// Where the gallery's artworks are kept.
enum Gallery {
    static let store: ArtworkStore = {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSTemporaryDirectory())
        return ArtworkStore.open(root: support.appendingPathComponent("gallery").path)
    }()
}

/// An artwork's thumbnail (saved with it) and its name.
private struct Thumbnail: View {
    let artwork: GalleryArtwork

    var body: some View {
        VStack(spacing: 8) {
            Group {
                if let picture = UIImage(contentsOfFile: (artwork.dir as NSString).appendingPathComponent("thumbnail.png")) {
                    Image(uiImage: picture)
                        .resizable()
                        .scaledToFit()
                } else {
                    Rectangle().fill(Color.white)
                }
            }
            .frame(height: 160)
            Text(artwork.name)
                .lineLimit(1)
        }
    }
}
