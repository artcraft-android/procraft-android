package io.github.artcraftandroid.procraft.canvas

import androidx.compose.ui.geometry.Offset
import io.github.artcraftandroid.procraft.core.TransformBox
import io.github.artcraftandroid.procraft.core.TransformHandle

/** The warp's control points a touch can take: the corners, or all 16 with the advanced mesh. */
fun shownMeshPoints(advanced: Boolean): List<Int> = io.github.artcraftandroid.procraft.core.shownMeshPoints(advanced).map { it.toInt() }

/** Canvas point ([x], [y]) through the view [m] (`[a, b, c, d, e, f]`), in window pixels. */
fun toScreen(x: Float, y: Float, m: List<Float>): Offset = Offset(m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])

/**
 * The transform box on screen, in window pixels: its corners (top left, top right, bottom right,
 * bottom left), the middles of its edges (top, right, bottom, left), the rotate node above it, the
 * node below it that turns the box alone, and the box's up direction (worked out by the shared core).
 */
class BoxOnScreen(val corners: List<Offset>, val edges: List<Offset>, val rotate: Offset, val turn: Offset, val up: Offset) {
    internal fun shared() = io.github.artcraftandroid.procraft.core.ScreenBox(
        corners.flatMap { listOf(it.x, it.y) },
        edges.flatMap { listOf(it.x, it.y) },
        listOf(rotate.x, rotate.y),
        listOf(turn.x, turn.y),
        listOf(up.x, up.y),
    )
}

private fun offsets(v: List<Float>): List<Offset> = v.chunked(2).map { (x, y) -> Offset(x, y) }

/** Where [box] (canvas pixels) shows through the view [m] (`[a, b, c, d, e, f]`); [density] is pixels per dp. */
fun boxOnScreen(box: TransformBox, m: List<Float>, density: Float): BoxOnScreen? {
    val s = io.github.artcraftandroid.procraft.core.screenBox(box.corners, m, density) ?: return null
    return BoxOnScreen(offsets(s.corners), offsets(s.edges), offsets(s.rotate)[0], offsets(s.turn)[0], offsets(s.up)[0])
}

/** In a warp: the control point a touch at window point [p] takes, otherwise the surface under it (a move). */
fun meshHandleAt(box: TransformBox, m: List<Float>, p: Offset, density: Float, advanced: Boolean): TransformHandle =
    io.github.artcraftandroid.procraft.core.meshHandleAt(box.mesh, m, p.x, p.y, density, advanced)

/** The handle a touch at window point [p] takes: the nearest node within reach, otherwise a move. */
fun handleAt(box: BoxOnScreen, p: Offset, density: Float): TransformHandle =
    io.github.artcraftandroid.procraft.core.handleAt(box.shared(), p.x, p.y, density)
