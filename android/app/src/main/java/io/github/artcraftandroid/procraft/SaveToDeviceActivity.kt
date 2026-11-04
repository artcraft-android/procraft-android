package io.github.artcraftandroid.procraft

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.DocumentsContract
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.result.contract.ActivityResultContracts
import java.io.File
import java.util.concurrent.Executors

/**
 * The share sheet's "Save to device": exported files written where the person picks with the
 * system's own picker (one file under a name it suggests, several into a folder), as the share
 * sheet elsewhere always offers. Shows nothing of its own.
 */
class SaveToDeviceActivity : ComponentActivity() {
    private val files by lazy { intent.getStringArrayExtra(EXTRA_PATHS).orEmpty().map(::File).filter { it.isFile } }
    private val mime by lazy { intent.getStringExtra(EXTRA_MIME) ?: "application/octet-stream" }

    // (the type is only known once the intent is: the picker's own intent made at launch)
    private val saveOne = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        val uri = result.data?.data
        if (result.resultCode != RESULT_OK || uri == null) finish() else write { copy(files[0], uri) }
    }

    private val saveInto = registerForActivityResult(ActivityResultContracts.OpenDocumentTree()) { tree ->
        if (tree == null) {
            finish()
            return@registerForActivityResult
        }
        write {
            val folder = DocumentsContract.buildDocumentUriUsingTree(tree, DocumentsContract.getTreeDocumentId(tree))
            files.all { f ->
                val made = DocumentsContract.createDocument(contentResolver, folder, mime, f.name)
                made != null && copy(f, made)
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (savedInstanceState != null) return
        when {
            files.isEmpty() -> finish()
            files.size == 1 -> saveOne.launch(
                Intent(Intent.ACTION_CREATE_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType(mime)
                    .putExtra(Intent.EXTRA_TITLE, files[0].name),
            )
            else -> saveInto.launch(null)
        }
    }

    /** Runs [work] off the main thread, then says if it failed and closes. */
    private fun write(work: () -> Boolean) {
        val app = applicationContext
        Executors.newSingleThreadExecutor().execute {
            val saved = runCatching(work).getOrDefault(false)
            runOnUiThread {
                if (!saved) Toast.makeText(app, "Couldn't save the file", Toast.LENGTH_LONG).show()
                finish()
            }
        }
    }

    /** [file] written whole into [uri]; false if it couldn't be. */
    private fun copy(file: File, uri: Uri): Boolean {
        val out = contentResolver.openOutputStream(uri, "wt") ?: return false
        out.use { o -> file.inputStream().use { it.copyTo(o) } }
        return true
    }

    companion object {
        private const val EXTRA_PATHS = "paths"
        private const val EXTRA_MIME = "mime"

        /** What the share sheet starts to save [files] (of type [mime]) to the device. */
        fun intent(context: Context, files: List<File>, mime: String): Intent =
            Intent(context, SaveToDeviceActivity::class.java)
                .putExtra(EXTRA_PATHS, files.map { it.absolutePath }.toTypedArray())
                .putExtra(EXTRA_MIME, mime)
    }
}
