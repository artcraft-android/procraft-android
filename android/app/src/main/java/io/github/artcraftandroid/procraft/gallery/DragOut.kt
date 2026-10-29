package io.github.artcraftandroid.procraft.gallery

import android.content.ClipData
import android.content.ClipDescription
import android.content.ContentProvider
import android.content.ContentValues
import android.content.Context
import android.database.Cursor
import android.database.MatrixCursor
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Point
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import android.view.View
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.core.CanvasSession
import java.io.File

/**
 * Artworks dragged out of the gallery into other apps. Each is offered in the formats the
 * settings prefer, as a file made only when the other app opens it.
 *
 * Its addresses are `content://<package>.drag/<format>/<the artwork's folder under the gallery>/<file name>`.
 */
class DragProvider : ContentProvider() {
    override fun onCreate() = true

    override fun getType(uri: Uri): String? = uri.pathSegments.firstOrNull()?.let(::mime)

    override fun query(uri: Uri, projection: Array<out String>?, selection: String?, selectionArgs: Array<out String>?, sortOrder: String?): Cursor {
        val name = uri.lastPathSegment?.let { File(it).name } ?: ""
        val made = context?.let { made(it, uri) }?.takeIf { it.exists() }
        return MatrixCursor(arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE)).apply { addRow(arrayOf<Any?>(name, made?.length())) }
    }

    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor? {
        val ctx = context ?: return null
        val segments = uri.pathSegments
        if (segments.size != 3 || mode != "r") return null
        val root = File(ctx.filesDir, "artworks").canonicalFile
        val dir = File(root, segments[1]).canonicalFile
        if (!dir.path.startsWith(root.path + File.separator) || !File(dir, "artwork.json").exists()) return null
        val out = made(ctx, uri) ?: return null
        // made again only if the artwork changed since
        if (out.lastModified() < File(dir, "artwork.json").lastModified() || !out.exists()) {
            if (!exportArtwork(dir, segments[0], out)) return null
        }
        return ParcelFileDescriptor.open(out, ParcelFileDescriptor.MODE_READ_ONLY)
    }

    override fun insert(uri: Uri, values: ContentValues?): Uri? = null

    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String>?) = 0

    override fun update(uri: Uri, values: ContentValues?, selection: String?, selectionArgs: Array<out String>?) = 0

    companion object {
        private val Extensions = mapOf(CanvasViewModel.OWN_LABEL to CanvasViewModel.OWN_EXTENSION, "PSD" to "psd", "JPEG" to "jpg", "PNG" to "png", "TIFF" to "tif")

        fun mime(format: String): String = when (format) {
            "PSD" -> "image/vnd.adobe.photoshop"
            "JPEG" -> "image/jpeg"
            "PNG" -> "image/png"
            "TIFF" -> "image/tiff"
            else -> "application/octet-stream"
        }

        /** Where the file for [uri] is made: one folder per format and artwork. */
        private fun made(ctx: Context, uri: Uri): File? {
            val segments = uri.pathSegments
            if (segments.size != 3) return null
            val folder = (segments[0] + "-" + segments[1]).map { if (it.isLetterOrDigit()) it else '_' }.joinToString("")
            return File(File(ctx.cacheDir, "drags/$folder").apply { mkdirs() }, File(segments[2]).name)
        }

        /** The address of artwork [a] (in the gallery at [root]) as [format]. */
        fun uri(ctx: Context, format: String, a: Artwork, root: File): Uri {
            val base = a.name.filter { it.isLetterOrDigit() || it in " _-" }.trim().ifEmpty { "Untitled Artwork" }
            return Uri.Builder().scheme("content").authority(ctx.packageName + ".drag")
                .appendPath(format).appendPath(a.dir.relativeTo(root).path).appendPath("$base.${Extensions[format] ?: "bin"}").build()
        }
    }
}

/**
 * Writes the artwork saved in [dir] to [out] as [format] (one of [CanvasViewModel.DRAG_FORMATS], or
 * of the gallery's still [CanvasViewModel.SHARE_FORMATS]).
 */
fun exportArtwork(dir: File, format: String, out: File, pdfQuality: Int = 100): Boolean = runCatching {
    CanvasSession.open(dir.path).use { s ->
        if (!s.prepareOffscreen()) return@runCatching false
        when (format) {
            // ours and the other app's are the same package
            CanvasViewModel.OWN_LABEL, CanvasViewModel.PACKAGE_LABEL ->
                s.exportPackage(out.path, System.nanoTime().toULong(), io.github.artcraftandroid.procraft.canvas.TimeLapse(dir).forPackage())
            "PDF" -> s.exportPdf(out.path, false, pdfQuality.toUByte())
            "PSD" -> s.exportPsd(out.path)
            "TIFF" -> s.exportTiff(out.path)
            "PNG" -> s.exportPng(out.path)
            "JPEG" -> {
                val png = File(out.path + ".png")
                s.exportPng(png.path)
                val ok = pngToJpeg(png, out)
                png.delete()
                return@runCatching ok
            }
            else -> return@runCatching false
        }
    }
    true
}.getOrDefault(false)

/** The picture in [png] flattened onto white as a JPEG in [jpg], keeping its colour space. */
fun pngToJpeg(png: File, jpg: File): Boolean {
    val picture = BitmapFactory.decodeFile(png.path) ?: return false
    val space = picture.colorSpace ?: android.graphics.ColorSpace.get(android.graphics.ColorSpace.Named.SRGB)
    val flat = Bitmap.createBitmap(picture.width, picture.height, Bitmap.Config.ARGB_8888, true, space)
    android.graphics.Canvas(flat).apply {
        drawColor(android.graphics.Color.WHITE)
        drawBitmap(picture, 0f, 0f, null)
    }
    jpg.outputStream().use { flat.compress(Bitmap.CompressFormat.JPEG, 92, it) }
    return true
}

/** What the gallery's own drags carry, so it can tell them from another app's. */
class GalleryDrag(val entry: GalleryEntry)

/** How opaque a dragged artwork's (or stack's) picture is. */
private const val DRAG_PICTURE_ALPHA = 204

/**
 * Starts dragging artwork [a] (in the gallery at [root]) from [view]: out to other apps in the
 * preferred file format (and the preferred picture format too when that file isn't a picture),
 * and within the gallery. Its picture is the thumbnail at [width] x [height] pixels, a little
 * see-through, held at [touch] (from its top left).
 */
fun startArtworkDrag(view: View, a: Artwork, root: File, width: Int, height: Int, touch: Point): Boolean {
    val ctx = view.context
    val prefs = ctx.getSharedPreferences("interface", Context.MODE_PRIVATE)
    val file = CanvasViewModel.DRAG_FORMATS.getOrElse(prefs.getInt(CanvasViewModel.DRAG_FORMAT, 0)) { "ProCraft" }
    val image = CanvasViewModel.DRAG_IMAGE_FORMATS.getOrElse(prefs.getInt(CanvasViewModel.DRAG_IMAGE_FORMAT, 0)) { "JPEG" }
    val formats = if (file in CanvasViewModel.DRAG_IMAGE_FORMATS) listOf(file) else listOf(file, image)
    val clip = ClipData(ClipDescription(a.name, formats.map(DragProvider::mime).toTypedArray()), ClipData.Item(DragProvider.uri(ctx, formats[0], a, root)))
    formats.drop(1).forEach { clip.addItem(ClipData.Item(DragProvider.uri(ctx, it, a, root))) }
    // opaque, so the picture keeps its own alpha rather than the system's
    val flags = View.DRAG_FLAG_GLOBAL or View.DRAG_FLAG_GLOBAL_URI_READ or View.DRAG_FLAG_OPAQUE
    return view.startDragAndDrop(clip, dragPicture(view, a.thumbnail, width, height, touch), GalleryDrag(a), flags)
}

/** Starts dragging stack [s] from [view], within the gallery only; its picture as an artwork's is, its top artwork's. */
fun startStackDrag(view: View, s: Stack, width: Int, height: Int, touch: Point): Boolean {
    val top = s.artworks.firstOrNull() ?: return false
    return view.startDragAndDrop(null, dragPicture(view, top.thumbnail, width, height, touch), GalleryDrag(s), View.DRAG_FLAG_OPAQUE)
}

/** A dragged thing's picture: [thumbnail] at [width] x [height] pixels on white with rounded corners, a little see-through, held at [touch]. */
private fun dragPicture(view: View, thumbnail: File, width: Int, height: Int, touch: Point): View.DragShadowBuilder {
    val thumb = BitmapFactory.decodeFile(thumbnail.path)
    val w = width.coerceAtLeast(1)
    val h = height.coerceAtLeast(1)
    val r = 8f * view.resources.displayMetrics.density
    return object : View.DragShadowBuilder(view) {
        override fun onProvideShadowMetrics(size: Point, at: Point) {
            size.set(w, h)
            at.set(touch.x.coerceIn(0, w - 1), touch.y.coerceIn(0, h - 1))
        }

        override fun onDrawShadow(canvas: android.graphics.Canvas) {
            val frame = android.graphics.RectF(0f, 0f, w.toFloat(), h.toFloat())
            val layer = canvas.saveLayerAlpha(frame, DRAG_PICTURE_ALPHA)
            canvas.clipPath(android.graphics.Path().apply { addRoundRect(frame, r, r, android.graphics.Path.Direction.CW) })
            canvas.drawColor(android.graphics.Color.WHITE)
            thumb?.let { canvas.drawBitmap(it, null, frame, android.graphics.Paint(android.graphics.Paint.FILTER_BITMAP_FLAG)) }
            canvas.restoreToCount(layer)
        }
    }
}
