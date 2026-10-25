package io.github.artcraftandroid.procraft.ui

import android.graphics.BitmapFactory
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.TimeLapse
import org.json.JSONArray
import java.io.File
import java.text.DateFormat
import java.util.Date
import kotlin.math.roundToInt
import kotlinx.coroutines.launch

private val ScreenFill = Color(0xFF1F1F1F)
private val RailFill = Color(0xFF272727)
private val CardFill = Color(0xFF252525)
private val Ink = Color(0xFFC1C1C1)
private val RailIdle = Color(0xFFAAAAAA)
private val PhotoFill = Color(0xFF303030)
private val PhotoGlyph = Color(0xFF4E4E4E)
private val Placeholder = Color(0xFF605F63)
private val SignLine = Color(0xFF494949)
private val CapsuleFill = Color(0xFF1A1A1A)
private val CapsuleEdge = Color(0xFF373737)
private val DoneBlue = Color(0xFF0190FF)
private val CardCorner = 25.dp

/** Where a wide screen's page starts and ends, beside the pages' card. */
private val WideContentStart = 358.dp
private val WideContentEnd = 28.dp
private val CardEdge = Color(0xFF373737)
private val PageInk = Color(0xFF797979)

/** The pages by icon and name on a card headed "Canvas Info" (a wide screen's), as the brush studio's. */
@Composable
private fun InfoPagesCard(page: Int, onPage: (Int) -> Unit) {
    val shape = RoundedCornerShape(20.dp)
    Column(Modifier.padding(start = 10.dp, top = 10.dp, bottom = 10.dp).width(320.dp).fillMaxHeight().clip(shape).background(RailFill).border(1.dp, CardEdge, shape)) {
        Text("Canvas Info", color = Ink, fontSize = 24.sp, fontWeight = FontWeight.Bold, maxLines = 1, modifier = Modifier.padding(start = 24.dp, top = 19.dp))
        Column(Modifier.padding(start = 8.dp, end = 8.dp, top = 17.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            InfoTabs.forEachIndexed { i, tab ->
                val chosen = i == page
                Row(
                    Modifier.fillMaxWidth().height(53.dp).clip(CircleShape).background(if (chosen) Pc.Accent else Color.Transparent).clickable { onPage(i) },
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Box(Modifier.width(60.dp), contentAlignment = Alignment.Center) {
                        Icon(tab.icon, contentDescription = null, tint = if (chosen) Color.White else RailIdle, modifier = Modifier.requiredSize(tab.size))
                    }
                    Text(tab.label, color = if (chosen) Color.White else PageInk, fontSize = 17.sp, maxLines = 1)
                }
            }
        }
    }
}

/** A page of the rail: its name, and its icon shown [size] across (each its own). */
private class InfoTab(val label: String, val icon: ImageVector, val size: Dp)

private val InfoTabs = listOf(
    InfoTab("About this artwork", PcIcons.InfoAbout, 39.dp),
    InfoTab("Dimensions", PcIcons.InfoDimensions, 51.dp),
    InfoTab("Layers", PcIcons.InfoLayers, 35.dp),
    InfoTab("Color profile", PcIcons.InfoProfile, 33.dp),
    InfoTab("Video settings", PcIcons.InfoVideo, 55.dp),
    InfoTab("Statistics", PcIcons.InfoStatistics, 35.dp),
)

/**
 * Canvas information, full screen: a rail of pages (about the artwork, its dimensions, layers,
 * colour profile, video and statistics), the artwork's name to edit as the title, Cancel and
 * Done.
 */
@Composable
fun CanvasInfo(vm: CanvasViewModel) {
    val session = vm.session ?: return
    val dir = vm.artworkFolder()
    val about = remember(dir) { dir?.let { File(it, "about") } }
    val info = remember { vm.artworkInfo() }
    var page by remember { mutableIntStateOf(0) }
    var name by remember { mutableStateOf(session.name()) }
    var author by remember { mutableStateOf(info?.author ?: "") }
    var photo by remember { mutableStateOf<ImageBitmap?>(about?.let { File(it, "photo.jpg") }?.takeIf { it.exists() }?.let { BitmapFactory.decodeFile(it.path)?.asImageBitmap() }) }
    var newPhoto by remember { mutableStateOf<android.net.Uri?>(null) }
    // the profile picked on its page, set on Done
    var profile by remember { mutableStateOf(session.profile()) }
    var askProfile by remember { mutableStateOf(false) }
    val signature = remember { mutableStateListOf<List<Offset>>().apply { addAll(readSignature(about)) } }
    val context = LocalContext.current
    val scope = androidx.compose.runtime.rememberCoroutineScope()
    val importer = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        uri?.let {
            scope.launch {
                // a profile of the canvas's kind is picked to change to
                vm.importProfile(it)?.takeIf { name -> io.github.artcraftandroid.procraft.core.isCmykProfile(name) == session.isCmyk() }?.let { name -> profile = name }
            }
        }
    }
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        if (uri != null) {
            newPhoto = uri
            photo = runCatching { context.contentResolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it) }?.asImageBitmap() }.getOrNull()
        }
    }
    fun close() {
        vm.canvasInfoOpen = false
    }
    fun done(asked: Boolean = false) {
        // a time-lapse recorded so far is in the old profile: asked first
        if (!asked && profile != session.profile() && vm.timeLapseInfo().first > 0) {
            askProfile = true
            return
        }
        vm.changeArtworkInfo(name, author)
        about?.let { folder ->
            folder.mkdirs()
            newPhoto?.let { uri -> runCatching { context.contentResolver.openInputStream(uri)?.use { input -> File(folder, "photo.jpg").outputStream().use { input.copyTo(it) } } } }
            writeSignature(folder, signature)
        }
        if (profile != session.profile()) vm.changeProfile(profile)
        close()
    }
    BackHandler { close() }
    // on a wide screen the pages are named on a card, the page and the name to its right
    val screen = androidx.compose.ui.platform.LocalConfiguration.current
    val wide = screen.screenWidthDp > screen.screenHeightDp
    Box(Modifier.fillMaxSize().background(ScreenFill).pointerInput(Unit) { detectTapGestures { } }) {
        if (wide) {
            InfoPagesCard(page) { page = it }
        } else {
            // the rail of pages
            Column(Modifier.width(81.dp).fillMaxHeight().background(RailFill).padding(start = 8.dp, top = 46.dp)) {
                InfoTabs.forEachIndexed { i, tab ->
                    val chosen = i == page
                    Box(
                        Modifier.size(65.dp, 53.dp).clip(CircleShape).background(if (chosen) Pc.Accent else Color.Transparent).clickable { page = i },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(tab.icon, contentDescription = tab.label, tint = if (chosen) Color.White else RailIdle, modifier = Modifier.requiredSize(tab.size))
                    }
                    Spacer(Modifier.height(3.dp))
                }
            }
        }
        // the name (centred over the page when wide), Cancel and Done
        BasicTextField(
            name,
            { name = it.take(256) },
            singleLine = true,
            textStyle = TextStyle(color = Ink, fontSize = 18.sp, fontWeight = FontWeight.Bold, textAlign = if (wide) androidx.compose.ui.text.style.TextAlign.Center else androidx.compose.ui.text.style.TextAlign.Start),
            cursorBrush = SolidColor(Color.White),
            modifier = if (wide) {
                Modifier.padding(start = WideContentStart, end = WideContentEnd).offset(y = 30.dp).fillMaxWidth()
            } else {
                Modifier.offset(x = 101.dp, y = 30.dp).width(400.dp)
            },
        )
        Row(Modifier.align(Alignment.TopEnd).padding(top = 20.dp, end = 20.dp), verticalAlignment = Alignment.CenterVertically) {
            // Cancel, with Import before it on the colour profile page
            Row(Modifier.height(43.dp).clip(CircleShape).background(CapsuleFill).border(1.dp, CapsuleEdge, CircleShape), verticalAlignment = Alignment.CenterVertically) {
                if (page == 3) CapsuleText("Import") { importer.launch(arrayOf("*/*")) }
                CapsuleText("Cancel") { close() }
            }
            Spacer(Modifier.width(13.dp))
            Box(Modifier.size(44.dp, 45.dp).clip(CircleShape).background(DoneBlue).clickable { done() }, contentAlignment = Alignment.Center) {
                Icon(PcIcons.DoneTick, contentDescription = "Done", tint = Color.White, modifier = Modifier.requiredSize(40.dp))
            }
        }
        Column(Modifier.padding(start = if (wide) WideContentStart else 105.dp, end = if (wide) WideContentEnd else 24.dp, top = 100.dp).verticalScroll(rememberScrollState())) {
            when (page) {
                0 -> {
                    AboutCard(photo, author, { author = it.take(256) }, { pick.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)) }, signature)
                    Spacer(Modifier.height(21.dp))
                    val created = info?.createdMs?.toLong()?.takeIf { it > 0 } ?: dir?.let { File(it, "artwork.json").lastModified() }?.takeIf { it > 0 }
                    val modified = dir?.let { File(it, "artwork.json").lastModified() }?.takeIf { it > 0 } ?: created
                    Rows(listOf("Date created" to (created?.let(::dateText) ?: "—"), "Date modified" to (modified?.let(::dateText) ?: "—")))
                }
                1 -> {
                    val c = session.info()
                    val dpi = c.dpi.coerceAtLeast(1f)
                    val mm = { px: UInt -> "${(px.toFloat() / dpi * 25.4f).roundToInt()}mm" }
                    Rows(listOf("Pixel width" to "${c.width}px", "Pixel height" to "${c.height}px"))
                    Spacer(Modifier.height(21.dp))
                    Rows(listOf("Physical width" to mm(c.width), "Physical height" to mm(c.height)))
                    Spacer(Modifier.height(21.dp))
                    Rows(listOf("DPI" to if (dpi % 1f == 0f) "${dpi.toInt()}" else "%.1f".format(dpi)))
                }
                2 -> {
                    val l = session.layerStats()
                    Rows(listOf("Maximum layers" to "${l.maximum}"))
                    Spacer(Modifier.height(21.dp))
                    Rows(listOf("Layers used" to "${l.used}", "Layers available" to "${(l.maximum.toInt() - l.used.toInt()).coerceAtLeast(0)}"))
                    Spacer(Modifier.height(21.dp))
                    Rows(listOf("Assisted layers" to "0", "Clipping layers" to "${l.clipping}", "Layer masks" to "${l.masks}", "Layer groups" to "${l.groups}"))
                }
                3 -> {
                    Rows(listOf("Current color profile" to session.profile()), tall = 44)
                    Text("Available color profiles", color = Ink, fontSize = 17.sp, modifier = Modifier.padding(start = 16.dp, top = 32.dp, bottom = 11.dp))
                    // the profiles of the canvas's kind: RGB, or CMYK
                    val names = if (session.isCmyk()) io.github.artcraftandroid.procraft.core.cmykProfiles() else io.github.artcraftandroid.procraft.core.colorProfiles()
                    ProfileRows(names, profile.takeIf { it != session.profile() }) { profile = it }
                }
                4 -> {
                    val (frames, bytes) = vm.timeLapseInfo()
                    val seconds = frames / TimeLapse.FPS
                    Rows(
                        listOf(
                            "Video length" to "%02d:%02d:%02d".format(seconds / 3600, seconds / 60 % 60, seconds % 60),
                            "Video quality" to "Good",
                            "Video resolution" to "720p",
                            "Video file size" to megabytes(bytes),
                            "Video codec" to "H.264",
                        ),
                    )
                }
                else -> {
                    val minutes = (info?.trackedMs?.toLong() ?: 0L) / 60_000L
                    Rows(
                        listOf(
                            "Total strokes made" to "${info?.strokes ?: 0u}",
                            "Tracked time" to "${minutes / 60}h ${minutes % 60}m",
                            "Total file size" to megabytes(dir?.walkTopDown()?.filter { it.isFile }?.sumOf { it.length() } ?: 0L),
                        ),
                    )
                }
            }
            Spacer(Modifier.height(40.dp))
        }
        Alert(
            if (askProfile) Unit else null,
            title = { "Change color profile?" },
            message = { "The time-lapse recorded so far was made in the old profile's colours." },
            action = "Change",
            destructive = false,
            cancel = "Don't change",
            onCancel = {
                askProfile = false
                profile = session.profile()
                done(asked = true)
            },
            onAction = {
                askProfile = false
                done(asked = true)
            },
        )
    }
}

/** One of the header capsule's words, a button. */
@Composable
private fun CapsuleText(label: String, onClick: () -> Unit) {
    Box(Modifier.fillMaxHeight().clickable(onClick = onClick).padding(horizontal = 13.dp), contentAlignment = Alignment.Center) {
        Text(label, color = Color(0xFFA5A5A5), fontSize = 17.sp)
    }
}

/** The profiles to pick from in a rounded card, a line between, [picked] ticked. */
@Composable
private fun ProfileRows(names: List<String>, picked: String?, onPick: (String) -> Unit) {
    // the profile picked to change to fills its row blue; the current one is not marked
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(CardCorner)).background(CardFill)) {
        names.forEachIndexed { i, name ->
            if (i > 0) Box(Modifier.fillMaxWidth().height(2.dp).background(ScreenFill))
            val on = name == picked
            Row(
                Modifier.fillMaxWidth().height(47.dp).background(if (on) Pc.Accent else Color.Transparent).clickable { onPick(name) }.padding(horizontal = 13.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(name, color = if (on) Color.White else Ink, fontSize = 14.sp, maxLines = 1, modifier = Modifier.weight(1f))
            }
        }
    }
}

/** Rows of a name and its value in a rounded card, a line between. */
@Composable
private fun Rows(rows: List<Pair<String, String>>, tall: Int = 50) {
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(CardCorner)).background(CardFill)) {
        rows.forEachIndexed { i, (label, value) ->
            if (i > 0) Box(Modifier.fillMaxWidth().height(1.dp).background(ScreenFill))
            Row(Modifier.fillMaxWidth().height(tall.dp).padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(label, color = Ink, fontSize = 17.sp, maxLines = 1, modifier = Modifier.weight(1f))
                Text(value, color = Ink, fontSize = 17.sp, maxLines = 1)
            }
        }
    }
}

/** The author: a photo (tap for another), "Made by" and the name, and a signature drawn on a line. */
@Composable
private fun AboutCard(photo: ImageBitmap?, author: String, onAuthor: (String) -> Unit, onPhoto: () -> Unit, signature: MutableList<List<Offset>>) {
    Box(Modifier.fillMaxWidth().height(300.dp).clip(RoundedCornerShape(CardCorner)).background(CardFill)) {
        Box(
            Modifier.align(Alignment.TopCenter).padding(top = 30.dp).size(97.dp).clip(CircleShape).background(PhotoFill).clickable(onClick = onPhoto),
            contentAlignment = Alignment.Center,
        ) {
            if (photo != null) Image(photo, contentDescription = "Author photo", contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
            else Icon(PcIcons.Person, contentDescription = "Add a photo", tint = PhotoGlyph, modifier = Modifier.size(48.dp))
        }
        Row(Modifier.align(Alignment.TopCenter).padding(top = 146.dp), verticalAlignment = Alignment.Bottom) {
            Text("Made by ", color = Ink, fontSize = 21.sp)
            Box {
                if (author.isEmpty()) Text("Name", color = Placeholder, fontSize = 21.sp)
                BasicTextField(
                    author,
                    onAuthor,
                    singleLine = true,
                    textStyle = TextStyle(color = Ink, fontSize = 21.sp),
                    cursorBrush = SolidColor(Color.White),
                    modifier = Modifier.width(if (author.isEmpty()) 60.dp else (author.length * 12 + 16).coerceAtMost(260).dp),
                )
            }
        }
        // the signature: drawn with a finger or pen over the dashed line
        Box(Modifier.align(Alignment.TopCenter).padding(top = 180.dp).size(401.dp, 121.dp)) {
            Canvas(
                Modifier.fillMaxSize().pointerInput(Unit) {
                    awaitEachGesture {
                        val down = awaitFirstDown()
                        down.consume()
                        val line = mutableListOf(Offset(down.position.x / size.width, down.position.y / size.height))
                        signature.add(line.toList())
                        while (true) {
                            val c = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                            if (!c.pressed) break
                            line.add(Offset(c.position.x / size.width, c.position.y / size.height))
                            signature[signature.size - 1] = line.toList()
                            c.consume()
                        }
                    }
                },
            ) {
                val lineY = 82.dp.toPx()
                val from = (size.width - 305.dp.toPx()) / 2f
                drawLine(SignLine, Offset(from, lineY), Offset(from + 305.dp.toPx(), lineY), 1.5.dp.toPx(), pathEffect = PathEffect.dashPathEffect(floatArrayOf(3.dp.toPx(), 4.dp.toPx())))
                for (stroke in signature) {
                    val path = Path()
                    stroke.forEachIndexed { i, p -> if (i == 0) path.moveTo(p.x * size.width, p.y * size.height) else path.lineTo(p.x * size.width, p.y * size.height) }
                    drawPath(path, Ink, style = Stroke(2.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round))
                }
            }
            Text("Sign here", color = PhotoGlyph, fontSize = 11.sp, modifier = Modifier.offset(x = 50.dp, y = 90.dp))
            // always there, at the line's end
            Box(Modifier.offset(x = 330.dp, y = 58.dp).size(22.dp).clip(CircleShape).background(PhotoGlyph).clickable { signature.clear() }, contentAlignment = Alignment.Center) {
                Icon(PcIcons.Cross, contentDescription = "Clear the signature", tint = CardFill, modifier = Modifier.size(14.dp))
            }
        }
    }
}

private fun dateText(ms: Long): String =
    DateFormat.getDateInstance(DateFormat.LONG).format(Date(ms)) + " at " + DateFormat.getTimeInstance(DateFormat.SHORT).format(Date(ms))

private fun megabytes(bytes: Long) = "%.2f MB".format(bytes / 1_000_000.0)

/** The signature kept in an artwork's about folder: strokes of points across its box, 0..1. */
private fun readSignature(about: File?): List<List<Offset>> = runCatching {
    val strokes = JSONArray(File(about, "signature.json").readText())
    List(strokes.length()) { i ->
        val pts = strokes.getJSONArray(i)
        List(pts.length() / 2) { j -> Offset(pts.getDouble(j * 2).toFloat(), pts.getDouble(j * 2 + 1).toFloat()) }
    }
}.getOrDefault(emptyList())

private fun writeSignature(about: File, strokes: List<List<Offset>>) {
    val out = JSONArray()
    for (s in strokes) out.put(JSONArray().apply { s.forEach { put(it.x.toDouble()); put(it.y.toDouble()) } })
    runCatching { File(about, "signature.json").writeText(out.toString()) }
}
