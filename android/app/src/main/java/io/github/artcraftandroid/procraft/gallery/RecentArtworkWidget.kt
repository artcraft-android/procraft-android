package io.github.artcraftandroid.procraft.gallery

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.view.View
import android.widget.RemoteViews
import io.github.artcraftandroid.procraft.MainActivity
import io.github.artcraftandroid.procraft.R
import java.io.File

/**
 * The home screen's Recent artwork widget: the picture of the artwork worked on last filling it (the app's mark
 * when there is none); a tap opens the app at that artwork.
 */
class RecentArtworkWidget : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
        val views = views(context)
        for (id in ids) manager.updateAppWidget(id, views)
    }

    companion object {
        /** What a tap on the widget asks of the app. */
        const val OPEN_RECENT = "io.github.artcraftandroid.procraft.OPEN_RECENT"

        /** The longest side the picture is given at (the home screen holds it in memory). */
        private const val PICTURE_MOST = 720

        /** Shows the artwork worked on last on every Recent artwork widget there is. */
        fun refresh(context: Context) {
            val manager = AppWidgetManager.getInstance(context)
            val ids = runCatching { manager.getAppWidgetIds(ComponentName(context, RecentArtworkWidget::class.java)) }.getOrNull()
            if (ids == null || ids.isEmpty()) return
            val views = views(context)
            for (id in ids) manager.updateAppWidget(id, views)
        }

        private fun views(context: Context): RemoteViews {
            val views = RemoteViews(context.packageName, R.layout.widget_recent)
            val picture = ArtworkStore(File(context.filesDir, "artworks")).mostRecent()?.thumbnail?.let(::picture)
            if (picture != null) {
                views.setImageViewBitmap(R.id.widget_picture, picture)
                views.setViewVisibility(R.id.widget_picture, View.VISIBLE)
                views.setViewVisibility(R.id.widget_mark, View.GONE)
            } else {
                views.setViewVisibility(R.id.widget_picture, View.GONE)
                views.setViewVisibility(R.id.widget_mark, View.VISIBLE)
            }
            val open = Intent(context, MainActivity::class.java).setAction(OPEN_RECENT).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
            views.setOnClickPendingIntent(R.id.widget_root, PendingIntent.getActivity(context, 0, open, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT))
            return views
        }

        /** The thumbnail read at most [PICTURE_MOST] across. */
        private fun picture(f: File): Bitmap? {
            if (!f.isFile) return null
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            BitmapFactory.decodeFile(f.path, bounds)
            var sample = 1
            while (maxOf(bounds.outWidth, bounds.outHeight) / (sample * 2) >= PICTURE_MOST) sample *= 2
            return runCatching { BitmapFactory.decodeFile(f.path, BitmapFactory.Options().apply { inSampleSize = sample }) }.getOrNull()
        }
    }
}
