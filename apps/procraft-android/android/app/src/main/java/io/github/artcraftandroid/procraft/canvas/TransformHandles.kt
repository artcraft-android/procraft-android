package io.github.artcraftandroid.procraft.canvas

import androidx.compose.ui.geometry.Offset
import io.github.artcraftandroid.procraft.core.TransformBox
import io.github.artcraftandroid.procraft.core.TransformHandle

/** How far the rotate node sits above the box's top edge, and the box-turning node below its bottom (dp). */
const val ROTATE_LIFT_DP = 34.5f
const val TURN_DROP_DP = 35.5f

/** How near a node a touch must land to take it (dp). */
private const val GRAB_RADIUS_DP = 37.5f

/** How near a warp's control point a touch must land to take it rather than the surface (dp). */
private const val MESH_GRAB_RADIUS_DP = 15f

/** The warp's control points a touch can take: the corners, or all 16 with the advanced mesh. */
fun shownMeshPoints(advanced: Boolean): List<Int> = if (advanced) (0 until 16).toList() else listOf(0, 3, 12, 15)

/** Canvas point ([x], [y]) through the view [m] (`[a, b, c, d, e, f]`), in window pixels. */
fun toScreen(x: Float, y: Float, m: List<Float>): Offset = Offset(m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])

/**
 * The transform box on screen, in window pixels: its corners (top left, top right, bottom right,
 * bottom left), the middles of its edges (top, right, bottom, left), the rotate node above it, the
 * node below it that turns the box alone, and the box's up direction.
 */
class BoxOnScreen(val corners: List<Offset>, val edges: List<Offset>, val rotate: Offset, val turn: Offset, val up: Offset)

/** Where [box] (canvas pixels) shows through the view [m] (`[a, b, c, d, e, f]`); [density] is pixels per dp. */
fun boxOnScreen(box: TransformBox, m: List<Float>, density: Float): BoxOnScreen? {
    val c = box.corners
    if (c.size < 8 || m.size < 6) return null
    val corners = List(4) { i -> toScreen(c[2 * i], c[2 * i + 1], m) }
    val edges = List(4) { (corners[it] + corners[(it + 1) % 4]) / 2f }
    val upward = edges[0] - edges[2]
    val length = upward.getDistance()
    val up = if (length > 1e-3f) upward / length else Offset(0f, -1f)
    // the box's right, a quarter turn clockwise from up
    val right = Offset(-up.y, up.x)
    val rotate = edges[0] + up * (ROTATE_LIFT_DP * density)
    val turn = edges[2] - up * (TURN_DROP_DP * density) + right * density
    return BoxOnScreen(corners, edges, rotate, turn, up)
}

/** In a warp: the control point a touch at window point [p] takes, otherwise the surface under it (a move). */
fun meshHandleAt(box: TransformBox, m: List<Float>, p: Offset, density: Float, advanced: Boolean): TransformHandle {
    val mesh = box.mesh
    if (mesh.size < 32 || m.size < 6) return TransformHandle.Move
    val reach = MESH_GRAB_RADIUS_DP * density
    val nearest = shownMeshPoints(advanced).minByOrNull { (toScreen(mesh[2 * it], mesh[2 * it + 1], m) - p).getDistance() }
        ?: return TransformHandle.Move
    val at = toScreen(mesh[2 * nearest], mesh[2 * nearest + 1], m)
    return if ((at - p).getDistance() <= reach) TransformHandle.Mesh(nearest.toUByte()) else TransformHandle.Move
}

/** The handle a touch at window point [p] takes: the nearest node within reach, otherwise a move. */
fun handleAt(box: BoxOnScreen, p: Offset, density: Float): TransformHandle {
    val reach = GRAB_RADIUS_DP * density
    val nodes = buildList {
        add(TransformHandle.Rotate to box.rotate)
        add(TransformHandle.TurnBox to box.turn)
        box.corners.forEachIndexed { i, c -> add(TransformHandle.Corner(i.toUByte()) to c) }
        box.edges.forEachIndexed { i, c -> add(TransformHandle.Edge(i.toUByte()) to c) }
    }
    val nearest = nodes.minByOrNull { (_, at) -> (at - p).getDistance() } ?: return TransformHandle.Move
    return if ((nearest.second - p).getDistance() <= reach) nearest.first else TransformHandle.Move
}
