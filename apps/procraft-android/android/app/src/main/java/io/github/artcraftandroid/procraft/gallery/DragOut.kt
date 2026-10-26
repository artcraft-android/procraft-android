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

/**
 * Starts dragging artwork [a] (in the gallery at [root]) from [view] out to other apps: the
 * preferred file format, and the preferred picture format too when that file isn't a picture.
 */
fun startArtworkDrag(view: View, a: Artwork, root: File): Boolean {
    val ctx = view.context
    val prefs = ctx.getSharedPreferences("interface", Context.MODE_PRIVATE)
    val file = CanvasViewModel.DRAG_FORMATS.getOrElse(prefs.getInt(CanvasViewModel.DRAG_FORMAT, 0)) { "ProCraft" }
    val image = CanvasViewModel.DRAG_IMAGE_FORMATS.getOrElse(prefs.getInt(CanvasViewModel.DRAG_IMAGE_FORMAT, 0)) { "JPEG" }
    val formats = if (file in CanvasViewModel.DRAG_IMAGE_FORMATS) listOf(file) else listOf(file, image)
    val clip = ClipData(ClipDescription(a.name, formats.map(DragProvider::mime).toTypedArray()), ClipData.Item(DragProvider.uri(ctx, formats[0], a, root)))
    formats.drop(1).forEach { clip.addItem(ClipData.Item(DragProvider.uri(ctx, it, a, root))) }
    val thumb = BitmapFactory.decodeFile(a.thumbnail.path)
    val shadow = object : View.DragShadowBuilder(view) {
        // the thumbnail, 160 dp on its long side, held at its middle
        private val side = 160f * ctx.resources.displayMetrics.density
        private val scale = thumb?.let { side / maxOf(it.width, it.height) } ?: 1f

        override fun onProvideShadowMetrics(size: Point, touch: Point) {
            val w = ((thumb?.width ?: 1) * scale).toInt().coerceAtLeast(1)
            val h = ((thumb?.height ?: 1) * scale).toInt().coerceAtLeast(1)
            size.set(w, h)
            touch.set(w / 2, h / 2)
        }

        override fun onDrawShadow(canvas: android.graphics.Canvas) {
            val t = thumb ?: return
            canvas.scale(scale, scale)
            canvas.drawBitmap(t, 0f, 0f, null)
        }
    }
    return view.startDragAndDrop(clip, shadow, null, View.DRAG_FLAG_GLOBAL or View.DRAG_FLAG_GLOBAL_URI_READ)
}
