package io.github.artcraftandroid.procraft.ui

import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.PlatformTextStyle
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.em

/** Touches leave no ripple: the reference's controls show presses in their own ways, or not at all. */
private object NoIndication : androidx.compose.foundation.IndicationNodeFactory {
    override fun create(interactionSource: androidx.compose.foundation.interaction.InteractionSource): androidx.compose.ui.node.DelegatableNode =
        object : androidx.compose.ui.Modifier.Node() {}

    override fun hashCode(): Int = -1

    override fun equals(other: Any?): Boolean = other === this
}

/** Letter spacing that brings the bundled face to the reference's text widths (measured over 18–24 pt labels). */
private const val TRACKING = -0.045f

/**
 * The app's theme: Inter (bundled in `assets/fonts`, OFL) for all text, with natural line heights
 * and no font padding, so text sits where the reference's labels do. Interface text keeps its size
 * whatever the system font scale, as the reference's does: sp equal dp inside the theme.
 */
@Composable
fun ProCraftTheme(content: @Composable () -> Unit) {
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
            LocalDensity provides Density(density.density, 1f),
            LocalTextStyle provides text,
            androidx.compose.foundation.LocalIndication provides NoIndication,
            content = content,
        )
    }
}
