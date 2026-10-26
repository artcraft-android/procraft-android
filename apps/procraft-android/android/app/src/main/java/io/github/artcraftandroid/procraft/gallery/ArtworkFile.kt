package io.github.artcraftandroid.procraft.gallery

import java.io.File
import java.io.InputStream
import java.util.zip.ZipInputStream

/**
 * An artwork as one file, as earlier versions shared it: its folder (the document, every layer, the
 * thumbnail and the time-lapse) in a zip. Still read; artworks are shared as packages now.
 */
object ArtworkFile {
    /**
     * Unzips an artwork file into the empty folder [dir]; false (and nothing kept) if it isn't
     * one. Only the files an artwork has are taken, and none outside the folder.
     */
    fun read(input: InputStream, dir: File): Boolean {
        val root = dir.canonicalFile
        var found = false
        ZipInputStream(input.buffered()).use { zip ->
            while (true) {
                val entry = zip.nextEntry ?: break
                val name = entry.name
                val allowed = name == "artwork.json" || name == "thumbnail.png" ||
                    (name.startsWith("layers/") && name.endsWith(".png")) ||
                    (name.startsWith("timelapse/") && (name.endsWith(".jpg") || name == "timelapse/off")) ||
                    name == "about/photo.jpg" || name == "about/signature.json"
                if (entry.isDirectory || !allowed || name.contains("..")) continue
                val target = File(root, name).canonicalFile
                if (!target.path.startsWith(root.path + File.separator)) continue
                target.parentFile?.mkdirs()
                target.outputStream().use { zip.copyTo(it) }
                if (name == "artwork.json") found = true
            }
        }
        if (!found) dir.deleteRecursively()
        return found
    }
}
