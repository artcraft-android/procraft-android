package io.github.artcraftandroid.procraft

import android.graphics.Color
import android.hardware.display.DisplayManager
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.runtime.snapshotFlow
import androidx.lifecycle.lifecycleScope
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.ProjectionPresentation
import kotlinx.coroutines.launch
import io.github.artcraftandroid.procraft.ui.AppRoot

/** How long a finger's touch stays where it landed with tap assistance on. */
private const val TAP_HOLD_MS = 300L

class MainActivity : ComponentActivity() {
    private val vm: CanvasViewModel by viewModels()

    // Project canvas: the presentation on a second display, and the displays coming and going
    private var projection: ProjectionPresentation? = null
    private val displays by lazy { getSystemService(DisplayManager::class.java) }
    private val displayListener = object : DisplayManager.DisplayListener {
        override fun onDisplayAdded(displayId: Int) = showProjection()
        override fun onDisplayRemoved(displayId: Int) = showProjection()
        override fun onDisplayChanged(displayId: Int) = Unit
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge(SystemBarStyle.dark(Color.TRANSPARENT), SystemBarStyle.dark(Color.TRANSPARENT))
        super.onCreate(savedInstanceState)
        io.github.artcraftandroid.procraft.canvas.TextRenderer.assets = assets
        io.github.artcraftandroid.procraft.canvas.TextRenderer.loadImported(java.io.File(filesDir, "fonts"))
        window.isNavigationBarContrastEnforced = false
        // wide colour, so a Display P3 canvas's colours show as they are around it too
        if (resources.configuration.isScreenWideColorGamut) window.colorMode = android.content.pm.ActivityInfo.COLOR_MODE_WIDE_COLOR_GAMUT
        hideSystemBars()
        // made here, not inside the flow's read-only snapshot, where its setup can't write state
        val model = vm
        setContent { AppRoot(model) }
        lifecycleScope.launch { snapshotFlow { model.projectCanvas }.collect { showProjection() } }
        importOpened(intent)
    }

    override fun onNewIntent(intent: android.content.Intent) {
        super.onNewIntent(intent)
        importOpened(intent)
    }

    /**
     * A file opened with the app, or shared to it, is imported into the gallery (once), which
     * shows: an open artwork is saved and closed first.
     */
    private fun importOpened(intent: android.content.Intent?) {
        intent ?: return
        // the home screen's Recent artwork widget: straight into the newest artwork
        if (intent.action == io.github.artcraftandroid.procraft.gallery.RecentArtworkWidget.OPEN_RECENT) {
            intent.action = null
            vm.openMostRecent()
            return
        }
        val uri = when (intent.action) {
            android.content.Intent.ACTION_VIEW -> intent.data
            android.content.Intent.ACTION_SEND ->
                androidx.core.content.IntentCompat.getParcelableExtra(intent, android.content.Intent.EXTRA_STREAM, android.net.Uri::class.java)
            else -> null
        } ?: return
        intent.action = null
        if (vm.screen == io.github.artcraftandroid.procraft.canvas.Screen.CANVAS) vm.closeArtwork()
        vm.importFile(uri)
    }

    override fun onStart() {
        super.onStart()
        vm.readAppSettings()
        vm.resumeTracking()
        displays.registerDisplayListener(displayListener, null)
        showProjection()
    }

    override fun onStop() {
        super.onStop()
        vm.saveNow()
        displays.unregisterDisplayListener(displayListener)
        projection?.dismiss()
        projection = null
    }

    /** Project canvas: while it is on, the canvas shows on the first second display there is. */
    private fun showProjection() {
        val display = if (vm.projectCanvas) displays.getDisplays(DisplayManager.DISPLAY_CATEGORY_PRESENTATION).firstOrNull() else null
        if (projection != null && projection?.display?.displayId == display?.displayId) return
        projection?.dismiss()
        projection = display?.let { d -> ProjectionPresentation(this, d, vm).takeIf { runCatching { it.show() }.isSuccess } }
    }

    // tap assistance: where a finger first landed, and when, while its touch is held there
    private var heldAt: android.graphics.PointF? = null
    private var heldSince = 0L

    /**
     * Tap assistance: for its first moments a finger's touch stays where it landed (its moves are
     * held back and its lift is put back there), so a tap that drifts still lands where it began.
     * More fingers, or a touch held longer, go on as they are.
     */
    override fun dispatchTouchEvent(e: android.view.MotionEvent): Boolean {
        if (!vm.tapAssistance || e.getToolType(0) != android.view.MotionEvent.TOOL_TYPE_FINGER) return super.dispatchTouchEvent(e)
        when (e.actionMasked) {
            android.view.MotionEvent.ACTION_DOWN -> {
                heldAt = android.graphics.PointF(e.x, e.y)
                heldSince = e.eventTime
            }
            android.view.MotionEvent.ACTION_POINTER_DOWN, android.view.MotionEvent.ACTION_CANCEL -> heldAt = null
            android.view.MotionEvent.ACTION_MOVE -> {
                if (heldAt != null && e.eventTime - heldSince <= TAP_HOLD_MS) return true
                heldAt = null
            }
            android.view.MotionEvent.ACTION_UP -> {
                val at = heldAt
                heldAt = null
                if (at != null) {
                    val held = android.view.MotionEvent.obtain(e).apply { setLocation(at.x, at.y) }
                    return super.dispatchTouchEvent(held).also { held.recycle() }
                }
            }
        }
        return super.dispatchTouchEvent(e)
    }

    /** A hardware keyboard's shortcuts on the canvas (keys no text field took). */
    override fun onKeyDown(keyCode: Int, event: android.view.KeyEvent): Boolean {
        if (event.repeatCount == 0 && vm.keyShortcut(keyCode, event.isCtrlPressed || event.isMetaPressed, event.isShiftPressed, event.isAltPressed)) return true
        return super.onKeyDown(keyCode, event)
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) hideSystemBars()
    }

    /** The canvas uses the whole screen; a swipe from the edge shows the bars for a moment. */
    private fun hideSystemBars() {
        val controller = WindowCompat.getInsetsController(window, window.decorView)
        controller.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        controller.hide(WindowInsetsCompat.Type.systemBars())
    }
}
