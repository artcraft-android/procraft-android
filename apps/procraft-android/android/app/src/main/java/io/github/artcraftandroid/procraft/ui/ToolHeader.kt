package io.github.artcraftandroid.procraft.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

private val HeaderLabel get() = if (Pc.light) Color(0xFF3A3A3A) else Color(0xFFC5C5C5)
private val ButtonLabel get() = if (Pc.light) Color(0xFF565656) else Color(0xFFA5A5A5)
private val ChosenLabel get() = if (Pc.light) Color(0xFF3A3A3A) else Color(0xFFC0C0C0)
private val CapsuleChosen get() = if (Pc.light) Color(0xFFA5A5A5) else Color(0xFF464646)
private val DoneBlue = Color(0xFF0190FF)

/** A full-screen tool's header is this tall. */
val ToolHeaderHeight = 85.dp

/** A button in a tool header's capsule: text, shown chosen, and what it does (none: just shown). */
class ToolButton(val label: String, val chosen: Boolean = false, val onClick: (() -> Unit)? = null)

/** A tool header's colours: its band, and its capsule's fill and edge. */
class ToolHeaderColors(val band: Color, val capsule: Color, val edge: Color)

/** Over the surround of a tool that edits the canvas: dark, or light with the light interface (then every tool's header is this one). */
val DarkToolHeader get() = if (Pc.light) ToolHeaderColors(Color(0xFFDFE2E3), Color(0xFFF5F8F9), Color(0xFFDCDEDF))
    else ToolHeaderColors(Color(0xFF1D1D1D), Color(0xFF1A1A1A), Color(0xFF373737))

/** Over a picture shown edge to edge. */
val LightToolHeader = ToolHeaderColors(Color(0xFF333333), Color(0xFF232323), Color(0xFF424242))

/**
 * The header full-screen tools share: the title on the left; on the right a capsule of text
 * buttons and the round blue Done.
 */
@Composable
fun ToolHeader(title: String, buttons: List<ToolButton>, onDone: () -> Unit, modifier: Modifier = Modifier, colors: ToolHeaderColors = DarkToolHeader) {
    // it fades in over the top bar as its mode begins
    val shown = androidx.compose.runtime.remember { androidx.compose.animation.core.Animatable(0f) }
    androidx.compose.runtime.LaunchedEffect(Unit) { shown.animateTo(1f, Motion.toolHeaderIn()) }
    Box(modifier.fillMaxWidth().height(ToolHeaderHeight).graphicsLayer { alpha = shown.value }.background(colors.band)) {
        Text(title, color = HeaderLabel, fontSize = 22.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.padding(start = 32.dp, top = 27.dp))
        Row(Modifier.align(Alignment.TopEnd).padding(top = 20.dp, end = 20.dp), verticalAlignment = Alignment.CenterVertically) {
            if (buttons.isNotEmpty()) {
                Row(Modifier.height(43.dp).clip(CircleShape).background(colors.capsule).border(1.dp, colors.edge, CircleShape), verticalAlignment = Alignment.CenterVertically) {
                    for (b in buttons) {
                        Box(
                            Modifier.height(43.dp).clip(CircleShape).background(if (b.chosen) CapsuleChosen else Color.Transparent)
                                .then(if (b.onClick != null) Modifier.clickable(onClick = b.onClick) else Modifier)
                                .padding(horizontal = 13.dp),
                            contentAlignment = Alignment.Center,
                        ) {
                            Text(b.label, color = if (b.chosen) ChosenLabel else ButtonLabel, fontSize = 17.sp, maxLines = 1)
                        }
                    }
                }
                Spacer(Modifier.width(13.dp))
            }
            Box(Modifier.size(44.dp).clip(CircleShape).background(DoneBlue).clickable(onClick = onDone), contentAlignment = Alignment.Center) {
                Icon(PcIcons.DoneTick, contentDescription = "Done", tint = Color.White, modifier = Modifier.requiredSize(40.dp))
            }
        }
    }
}
