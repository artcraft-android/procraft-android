package io.github.artcraftandroid.procraft.ui

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.net.Uri
import androidx.compose.ui.draganddrop.DragAndDropEvent
import androidx.compose.ui.draganddrop.toAndroidDragEvent

/** Whether [this] brings files from another app (a drag of our own carries its state). */
internal fun DragAndDropEvent.fromElsewhere(): Boolean {
    val d = toAndroidDragEvent()
    if (d.localState != null) return false
    val description = d.clipDescription ?: return false
    return (0 until description.mimeTypeCount).any { !description.getMimeType(it).startsWith("text/") }
}

/** The files [this] dropped, readable from now on: the app they come from lets [activity] read them. */
internal fun DragAndDropEvent.droppedFiles(activity: Activity?): List<Uri> {
    val d = toAndroidDragEvent()
    val clip = d.clipData ?: return emptyList()
    activity?.requestDragAndDropPermissions(d)
    return (0 until clip.itemCount).mapNotNull { clip.getItemAt(it).uri }
}

/** The activity this context belongs to, if any. */
internal fun Context.activity(): Activity? =
    generateSequence(this) { (it as? ContextWrapper)?.baseContext }.filterIsInstance<Activity>().firstOrNull()
