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
import io.github.artcraftandroid.procraft.canvas.penKeyButton
import kotlinx.coroutines.launch
import io.github.artcraftandroid.procraft.ui.AppRoot

/** How long a finger's touch stays where it landed with tap assistance on. */
private const val TAP_HOLD_MS = 300L

/** How far it may drift meanwhile and still be held (dp): farther, it is a scroll or a drag and moves. */
private const val TAP_DRIFT_DP = 24f

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
        // text's fonts given to the core, off the main thread
        io.github.artcraftandroid.procraft.canvas.TextRenderer.prepare(assets, java.io.File(filesDir, "fonts"))
        window.isNavigationBarContrastEnforced = false
        // wide colour, so a Display P3 canvas's colours show as they are around it too
        if (resources.configuration.isScreenWideColorGamut) window.colorMode = android.content.pm.ActivityInfo.COLOR_MODE_WIDE_COLOR_GAMUT
        hideSystemBars()
        // which graphics backend the app is trying, kept before the model makes the GPU device: a
        // driver that crashes the app then sends the next start to the next backend
        io.github.artcraftandroid.procraft.core.setGpuRecordFolder(filesDir.path)
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
        // a plugin is asked about where the app stands: it needs no artwork closed
        if (vm.isPluginFile(uri)) {
            vm.offerPlugin(uri)
            return
        }
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

    /** The system short of memory, or the app out of sight: the canvas lets go of what it keeps only to be quicker. */
    @Suppress("DEPRECATION")
    override fun onTrimMemory(level: Int) {
        super.onTrimMemory(level)
        if (level >= android.content.ComponentCallbacks2.TRIM_MEMORY_RUNNING_LOW) vm.session?.trimMemory()
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
     * More fingers, a touch held longer, or one gone farther than a drift (a list scrolled) go on
     * as they are.
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
                val at = heldAt
                val drift = TAP_DRIFT_DP * resources.displayMetrics.density
                if (at != null && e.eventTime - heldSince <= TAP_HOLD_MS && kotlin.math.hypot(e.x - at.x, e.y - at.y) <= drift) return true
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
        // a pen's side button that comes as a key
        val penButton = event.device?.let { penKeyButton(keyCode, it.sources or event.source, it.keyboardType, it.name) }
        if (penButton != null && (event.repeatCount > 0 || vm.penKey(penButton))) return true
        if (isAltKey(keyCode) && event.repeatCount == 0 && !event.isCtrlPressed && vm.modifyKey(true)) return true
        if (event.repeatCount == 0 && vm.keyShortcut(keyCode, event.isCtrlPressed || event.isMetaPressed, event.isShiftPressed, event.isAltPressed)) return true
        return super.onKeyDown(keyCode, event)
    }

    override fun onKeyUp(keyCode: Int, event: android.view.KeyEvent): Boolean {
        if (isAltKey(keyCode) && vm.modifyHeld && vm.modifyKey(false)) return true
        return super.onKeyUp(keyCode, event)
    }

    private fun isAltKey(keyCode: Int) = keyCode == android.view.KeyEvent.KEYCODE_ALT_LEFT || keyCode == android.view.KeyEvent.KEYCODE_ALT_RIGHT

    /** The keyboard shortcuts for the system's list of them (Meta+/, or Help's Keyboard shortcuts): the core's. */
    override fun onProvideKeyboardShortcuts(
        data: MutableList<android.view.KeyboardShortcutGroup>,
        menu: android.view.Menu?,
        deviceId: Int,
    ) {
        super.onProvideKeyboardShortcuts(data, menu, deviceId)
        for (group in io.github.artcraftandroid.procraft.core.keyShortcutList(false, false)) {
            val items = group.rows.flatMap { row ->
                row.combos.mapNotNull { c ->
                    val code = keyCodeNamed(c.key) ?: return@mapNotNull null
                    val modifiers = (if (c.command) android.view.KeyEvent.META_CTRL_ON else 0) or
                        (if (c.shift) android.view.KeyEvent.META_SHIFT_ON else 0) or
                        (if (c.alt) android.view.KeyEvent.META_ALT_ON else 0)
                    android.view.KeyboardShortcutInfo(row.does, code, modifiers)
                }
            }
            if (items.isNotEmpty()) data.add(android.view.KeyboardShortcutGroup(group.heading, items))
        }
    }

    /** The key code of a key as the core's shortcuts name it ("z", "[", "delete", "left"). */
    private fun keyCodeNamed(name: String): Int? = when {
        name.length == 1 && name[0] in 'a'..'z' -> android.view.KeyEvent.KEYCODE_A + (name[0] - 'a')
        name.length == 1 && name[0] in '0'..'9' -> android.view.KeyEvent.KEYCODE_0 + (name[0] - '0')
        name == "[" -> android.view.KeyEvent.KEYCODE_LEFT_BRACKET
        name == "]" -> android.view.KeyEvent.KEYCODE_RIGHT_BRACKET
        name == ";" -> android.view.KeyEvent.KEYCODE_SEMICOLON
        name == "space" -> android.view.KeyEvent.KEYCODE_SPACE
        name == "delete" -> android.view.KeyEvent.KEYCODE_FORWARD_DEL
        name == "backspace" -> android.view.KeyEvent.KEYCODE_DEL
        name == "left" -> android.view.KeyEvent.KEYCODE_DPAD_LEFT
        name == "right" -> android.view.KeyEvent.KEYCODE_DPAD_RIGHT
        name == "up" -> android.view.KeyEvent.KEYCODE_DPAD_UP
        name == "down" -> android.view.KeyEvent.KEYCODE_DPAD_DOWN
        else -> null
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
