package io.github.artcraftandroid.procraft.ui

import android.content.res.Configuration
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.PlatformTextStyle
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.em
import io.github.artcraftandroid.procraft.core.Plugins

/** Touches leave no ripple: the controls show presses in their own ways, or not at all. */
private object NoIndication : androidx.compose.foundation.IndicationNodeFactory {
    override fun create(interactionSource: androidx.compose.foundation.interaction.InteractionSource): androidx.compose.ui.node.DelegatableNode =
        object : androidx.compose.ui.Modifier.Node() {}

    override fun hashCode(): Int = -1

    override fun equals(other: Any?): Boolean = other === this
}

/** Letter spacing that brings the bundled face to the measured text widths (over 18–24 pt labels). */
private const val TRACKING = -0.045f

/**
 * The app's theme: Inter (bundled in `assets/fonts`, OFL) for all text, with natural line heights
 * and no font padding, so text sits where the measured labels do. Interface text keeps its size
 * whatever the system font scale: sp equal dp inside the theme.
 */
@Composable
fun ProCraftTheme(interfaceSize: Float = 1f, content: @Composable () -> Unit) {
    val assets = LocalContext.current.assets
    val text = remember(assets) {
        val family = FontFamily(
            Font("fonts/Inter-Regular.ttf", assets, FontWeight.Normal),
            Font("fonts/Inter-Medium.ttf", assets, FontWeight.Medium),
            Font("fonts/Inter-SemiBold.ttf", assets, FontWeight.SemiBold),
            Font("fonts/Inter-Bold.ttf", assets, FontWeight.Bold),
        )
        TextStyle(fontFamily = family, letterSpacing = TRACKING.em, platformStyle = PlatformTextStyle(includeFontPadding = false))
    }
    val typography = remember(text) {
        val t = Typography()
        Typography(
            displayLarge = t.displayLarge.merge(text),
            displayMedium = t.displayMedium.merge(text),
            displaySmall = t.displaySmall.merge(text),
            headlineLarge = t.headlineLarge.merge(text),
            headlineMedium = t.headlineMedium.merge(text),
            headlineSmall = t.headlineSmall.merge(text),
            titleLarge = t.titleLarge.merge(text),
            titleMedium = t.titleMedium.merge(text),
            titleSmall = t.titleSmall.merge(text),
            bodyLarge = t.bodyLarge.merge(text),
            bodyMedium = t.bodyMedium.merge(text),
            bodySmall = t.bodySmall.merge(text),
            labelLarge = t.labelLarge.merge(text),
            labelMedium = t.labelMedium.merge(text),
            labelSmall = t.labelSmall.merge(text),
        )
    }
    MaterialTheme(colorScheme = darkColorScheme(primary = Pc.Accent), typography = typography) {
        val density = LocalDensity.current
        CompositionLocalProvider(
            LocalDensity provides Density(density.density * interfaceSize, 1f),
            LocalConfiguration provides sizedScreen(interfaceSize),
            LocalSizeSettled provides sizeSettled(interfaceSize),
            LocalTextStyle provides text,
            androidx.compose.foundation.LocalIndication provides NoIndication,
            content = content,
        )
    }
}

/** Moves a frame after the interface's size does: see [sizeSettled]. */
private val LocalSizeSettled = staticCompositionLocalOf { 0 }

/**
 * Counts the sizes the interface has settled at. A BoxWithConstraints gives its content the size
 * in dp through the density of its last measuring, and as the window stays as it is nothing
 * composes it again after that: it would go on with the old size until the next turn of the
 * screen. Changing a static local a frame on, measured by then, has all of it read the new one.
 */
@Composable
private fun sizeSettled(interfaceSize: Float): Int {
    var settled by remember { mutableIntStateOf(0) }
    var last by remember { mutableFloatStateOf(interfaceSize) }
    LaunchedEffect(interfaceSize) {
        if (interfaceSize != last) {
            last = interfaceSize
            withFrameNanos { }
            settled++
        }
    }
    return settled
}

/** How big the interface shows with [chosen] set: as far as the window has room. */
@Composable
fun fittedInterfaceSize(plugins: Plugins, changes: Any? = null): Float {
    val screen = LocalConfiguration.current
    return remember(plugins, changes, screen.screenWidthDp, screen.screenHeightDp) {
        plugins.interfaceScale(screen.screenWidthDp.toFloat(), screen.screenHeightDp.toFloat())
    }
}

/** The screen as the interface measures it at [interfaceSize]: as many of its dp as fit across it. */
@Composable
private fun sizedScreen(interfaceSize: Float): Configuration {
    val screen = LocalConfiguration.current
    return remember(screen, interfaceSize) {
        if (interfaceSize == 1f) {
            screen
        } else {
            Configuration(screen).apply {
                screenWidthDp = (screenWidthDp / interfaceSize).toInt()
                screenHeightDp = (screenHeightDp / interfaceSize).toInt()
                smallestScreenWidthDp = (smallestScreenWidthDp / interfaceSize).toInt()
            }
        }
    }
}

/**
 * The text scale the popups made before [PcPopup] were matched at: they ran at
 * the Legion's system font scale (0.85) then, so their sizes are set for it. Kept for them, now
 * on any device.
 */
const val MATCHED_POPUP_TEXT = 0.85f

/**
 * A popup whose text keeps one size whatever the system font scale. A popup is a window of its
 * own, composed with the system's font scale, so its text followed it where the rest of the
 * interface doesn't; this hands it the theme's density, its text scaled by [textScale], and the
 * theme's screen.
 */
@Composable
fun PcPopup(
    alignment: androidx.compose.ui.Alignment = androidx.compose.ui.Alignment.TopStart,
    offset: androidx.compose.ui.unit.IntOffset = androidx.compose.ui.unit.IntOffset(0, 0),
    onDismissRequest: (() -> Unit)? = null,
    properties: androidx.compose.ui.window.PopupProperties = androidx.compose.ui.window.PopupProperties(),
    textScale: Float = 1f,
    content: @Composable () -> Unit,
) {
    val density = Density(LocalDensity.current.density, textScale)
    val screen = LocalConfiguration.current
    androidx.compose.ui.window.Popup(alignment, offset, onDismissRequest, properties) {
        CompositionLocalProvider(LocalDensity provides density, LocalConfiguration provides screen, content = content)
    }
}

/** [PcPopup] placed by a [popupPositionProvider]. */
@Composable
fun PcPopup(
    popupPositionProvider: androidx.compose.ui.window.PopupPositionProvider,
    onDismissRequest: (() -> Unit)? = null,
    properties: androidx.compose.ui.window.PopupProperties = androidx.compose.ui.window.PopupProperties(),
    textScale: Float = 1f,
    content: @Composable () -> Unit,
) {
    val density = Density(LocalDensity.current.density, textScale)
    val screen = LocalConfiguration.current
    androidx.compose.ui.window.Popup(popupPositionProvider, onDismissRequest, properties) {
        CompositionLocalProvider(LocalDensity provides density, LocalConfiguration provides screen, content = content)
    }
}
