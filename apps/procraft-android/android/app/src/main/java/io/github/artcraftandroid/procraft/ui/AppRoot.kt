package io.github.artcraftandroid.procraft.ui

import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.ui.viewinterop.AndroidView
import io.github.artcraftandroid.procraft.canvas.Adjustment
import io.github.artcraftandroid.procraft.canvas.CanvasSurfaceView
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.Panel
import io.github.artcraftandroid.procraft.canvas.Screen
import io.github.artcraftandroid.procraft.canvas.SelectionPopup
import io.github.artcraftandroid.procraft.canvas.TransformPopup
import io.github.artcraftandroid.procraft.core.LayerInfo
import io.github.artcraftandroid.procraft.core.ToolKind

/** The canvas screen: the canvas, the bars over it, and the panel that is open. */
@Composable
fun AppRoot(vm: CanvasViewModel) {
    ProCraftTheme {
        if (vm.screen == Screen.GALLERY) {
            ZoomingGallery(vm, overlay = false)
            return@ProCraftTheme
        }
        // back closes an open panel first, then leaves the artwork for the gallery
        BackHandler {
            when {
                vm.guideEditing -> vm.closeGuideEditor(keep = false)
                vm.lightingStudioOpen -> vm.closeLightingStudio(keep = false)
                vm.studioOpen -> vm.closeStudio(keep = false)
                vm.transformPopup != TransformPopup.NONE -> vm.transformPopup = TransformPopup.NONE
                vm.selectionPopup != SelectionPopup.NONE -> vm.selectionPopup = SelectionPopup.NONE
                vm.selecting -> vm.closeSelecting()
                vm.adjustment != null -> vm.endAdjustment(true)
                vm.transforming -> vm.endTransform(true)
                vm.panel != Panel.NONE -> vm.closePanels()
                else -> vm.closeArtwork()
            }
        }
        // panels over the canvas are frosted
        CompositionLocalProvider(LocalBackdrops provides vm.backdrops) {
            BoxWithConstraints(Modifier.fillMaxSize().background(Pc.Surround)) {
                AndroidView(factory = { CanvasSurfaceView(it, vm) }, modifier = Modifier.fillMaxSize())
                if (vm.transforming) TransformOverlay(vm)
                if (vm.selecting) SelectionOverlay(vm, Pc.BarHeight)
                if (vm.panel != Panel.NONE) {
                    // a tap outside an open panel closes it (and does not paint)
                    Box(Modifier.fillMaxSize().pointerInput(Unit) { detectTapGestures { vm.closePanels() } })
                }
                vm.picking?.let { pick ->
                    Loupe(pick, CanvasSpace.color(vm.colorRgb()))
                    if (!vm.barsHidden) PickingBar(Modifier.offset(y = Pc.BarHeight))
                }
                // the bars fade out as the interface hides, and back
                val barsShown = androidx.compose.animation.core.animateFloatAsState(if (vm.barsHidden) 0f else 1f, Motion.bars(), label = "bars").value
                // with the interface hidden, a button in the top left brings it back
                if (barsShown < 1f) {
                    Box(
                        Modifier.offset(x = 20.dp).size(44.dp).graphicsLayer { alpha = 1f - barsShown }
                            .pointerInput(Unit) { detectTapGestures { if (vm.barsHidden) vm.toggleBars() } },
                        contentAlignment = Alignment.Center,
                    ) {
                        // its glyph is drawn wider than it is tall: 50 units shown 36.7 by 32
                        androidx.compose.foundation.Image(
                            androidx.compose.ui.graphics.vector.rememberVectorPainter(PcIcons.FullScreen),
                            contentDescription = "Show the interface",
                            contentScale = androidx.compose.ui.layout.ContentScale.FillBounds,
                            colorFilter = androidx.compose.ui.graphics.ColorFilter.tint(Pc.SideGlyph),
                            modifier = Modifier.offset(y = 0.3.dp).requiredSize(36.7.dp, 32.dp),
                        )
                    }
                }
                // a slider's panel kept up goes with the next touch anywhere else
                if (vm.sliderPanelKept && vm.sliderPanel != null) {
                    Box(
                        Modifier.fillMaxSize().pointerInput(Unit) {
                            awaitEachGesture {
                                awaitFirstDown().consume()
                                vm.sliderPanel = null
                                vm.sliderPanelKept = false
                            }
                        },
                    )
                }
                if (barsShown > 0f) {
                    // the top bar slides up off the screen and the side bar out to its side as they fade
                    val fade = Modifier.graphicsLayer { alpha = barsShown }
                    val up = Modifier.graphicsLayer { translationY = -size.height * (1f - barsShown) }
                    val aside = Modifier.graphicsLayer { translationX = size.width * (1f - barsShown) * if (vm.rightHanded) 1f else -1f }
                    TopBar(vm, Modifier.align(Alignment.TopCenter).then(up).then(fade))
                    // a mode taking the screen slides it off its edge, and back when it ends
                    androidx.compose.animation.AnimatedVisibility(
                        vm.showSideBar && !vm.transforming && !vm.selecting && !vm.guideEditing && !vm.lightingStudioOpen && vm.cropRect == null && (vm.adjustment == null || vm.adjustPencil),
                        Modifier.align(if (vm.rightHanded) Alignment.TopEnd else Alignment.TopStart).offset(y = maxHeight * Pc.SIDE_BAR_CENTRE - Pc.SideBarHeight / 2),
                        enter = Motion.sideBarIn(vm.rightHanded),
                        exit = Motion.sideBarOut(vm.rightHanded),
                    ) {
                        SideBar(vm, aside.then(fade), right = vm.rightHanded)
                    }
                }
                NoticeBand(vm)
                // each panel hangs from its button: colour, layers, then the tool that opened the library;
                // it grows out of its arrow as it opens and fades as it closes
                PanelPresence(vm.panel, Panel.NONE, fading = vm.panelFading, onFaded = { vm.panelFading = false }) { shown, alpha, scale, origin ->
                    val (width, button) = when (shown) {
                        Panel.LAYERS -> (if (vm.backgroundPicking) 376.dp else 384.dp) to 1
                        Panel.COLOR -> 376.dp to 0
                        Panel.BRUSHES -> 440.dp to when (vm.tool) {
                            ToolKind.ERASE -> 2
                            ToolKind.SMUDGE -> 3
                            else -> 4
                        }
                        Panel.ACTIONS -> 390.dp to 0
                        Panel.ADJUSTMENTS -> 330.dp to 1
                        Panel.NONE -> 0.dp to 0
                    }
                    val fromLeft = shown == Panel.ACTIONS || shown == Panel.ADJUSTMENTS
                    val buttonX = if (fromLeft) Pc.actionCentre(button, maxWidth) else maxWidth - Pc.toolCentreFromRight(button, maxWidth)
                    // panels keep clear of the side bar, on whichever side it is
                    val clear = if (vm.showSideBar) Pc.SideBarWidth + Pc.PopoverGap else Pc.PopoverGap
                    val place = if (vm.rightHanded) popoverPlace(width, buttonX, maxWidth, minRight = clear) else popoverPlace(width, buttonX, maxWidth, clear)
                    val pivot = with(androidx.compose.ui.platform.LocalDensity.current) {
                        origin + androidx.compose.ui.geometry.Offset((place.left + place.arrowX).toPx(), Pc.BarHeight.toPx())
                    }
                    val at = Modifier.offset(x = place.left, y = Pc.BarHeight).graphicsLayer {
                        this.alpha = alpha
                        scaleX = scale
                        scaleY = scale
                        transformOrigin = androidx.compose.ui.graphics.TransformOrigin(place.arrowX / width, 0f)
                    }
                    ProvidePanelLook(PanelLook(alpha, scale, pivot)) {
                        when (shown) {
                            Panel.LAYERS -> if (vm.backgroundPicking) {
                                ColorPanel(vm, place.arrowX, maxHeight, at, background = true)
                                // closed any way at all, the colour is kept
                                androidx.compose.runtime.DisposableEffect(Unit) { onDispose { vm.endBackgroundPick() } }
                            } else {
                                LayersPanel(vm, place.arrowX, at.heightIn(max = maxHeight - Pc.BarHeight - Pc.PopoverGap))
                            }
                            Panel.COLOR -> ColorPanel(vm, place.arrowX, maxHeight, at)
                            Panel.BRUSHES -> BrushLibrary(vm, place.arrowX, at.height(maxHeight - Pc.BarHeight - 25.dp))
                            Panel.ACTIONS -> ActionsPanel(vm, place.arrowX, at)
                            Panel.ADJUSTMENTS -> AdjustmentsPanel(vm, place.arrowX, at.heightIn(max = maxHeight - Pc.BarHeight - Pc.PopoverGap))
                            Panel.NONE -> Unit
                        }
                    }
                }
                // the bottom toolbars rise from below the screen, and drop away
                androidx.compose.animation.AnimatedVisibility(
                    vm.transforming && !vm.barsHidden,
                    Modifier.align(Alignment.BottomCenter),
                    enter = Motion.toolbarIn(),
                    exit = Motion.toolbarOut(),
                ) { TransformToolbar(vm, maxWidth) }
                if (vm.transforming && !vm.barsHidden) {
                    TransformPopover(vm, maxWidth, maxHeight)
                    TransformNumeric(vm)
                }
                BrushCursor(vm)
                ReferenceWindow(vm)
                TouchCompanion(vm)
                CropOverlay(vm)
                if (vm.textInfo != null) TextEditor(vm)
                RasterizePrompt(vm)
                vm.animatedExport?.let { AnimatedExport(vm, it) }
                vm.modelAnimatedExport?.let { ModelAnimatedExport(vm, it) }
                // full-screen pages slide up over everything, and back down
                androidx.compose.animation.AnimatedVisibility(
                    vm.canvasInfoOpen,
                    enter = androidx.compose.animation.slideInVertically(Motion.studioIn()) { it },
                    exit = androidx.compose.animation.slideOutVertically(Motion.studioOut()) { it },
                ) { Box(Modifier.fillMaxSize()) { CanvasInfo(vm) } }
                androidx.compose.animation.AnimatedVisibility(
                    vm.gestureControlsOpen,
                    enter = androidx.compose.animation.slideInVertically(Motion.studioIn()) { it },
                    exit = androidx.compose.animation.slideOutVertically(Motion.studioOut()) { it },
                ) { Box(Modifier.fillMaxSize()) { GestureControls(vm) } }
                BlockedDialog(vm)
                // a passing message goes in the band under the top bar, as what undo and redo did
                vm.message?.let { text ->
                    androidx.compose.runtime.LaunchedEffect(text) {
                        vm.showNotice(text)
                        vm.message = null
                    }
                }
                androidx.compose.animation.AnimatedVisibility(
                    vm.timeLapseReplaying,
                    Modifier.matchParentSize(),
                    enter = androidx.compose.animation.fadeIn(Motion.replayFade()),
                    exit = androidx.compose.animation.fadeOut(Motion.replayFade()),
                ) { TimeLapseReplay(vm) }
                // the gallery zooming into the artwork as it opens, over everything
                ZoomingGallery(vm, overlay = true)
                GuideOverlay(vm)
                if (vm.adjustment == Adjustment.CLONE) ClonePuck(vm)
                QuickShapeHandles(vm)
                if (!vm.barsHidden) AnimationBar(vm)
                CopyPastePanel(vm)
                QuickMenu(vm)
                LayerChoice(vm)
                if (!vm.barsHidden) QuickShapeBar(vm, Modifier.offset(y = Pc.BarHeight))
                if (vm.adjustment != null && !vm.barsHidden) {
                    AdjustmentBar(vm, Modifier.offset(y = Pc.BarHeight))
                    AdjustModes(vm, Modifier.align(Alignment.TopCenter).offset(y = Pc.BarHeight + 52.dp))
                    AdjustmentMenu(vm)
                }
                // the adjustment's own panel rises from below the screen, and drops away
                val bottomPanel = vm.adjustment?.takeIf { !vm.barsHidden && it in BottomPanels }
                val shownPanel = androidx.compose.runtime.remember { arrayOfNulls<Adjustment>(1) }
                if (bottomPanel != null) shownPanel[0] = bottomPanel
                androidx.compose.animation.AnimatedVisibility(
                    bottomPanel != null,
                    Modifier.align(Alignment.BottomCenter),
                    enter = Motion.toolbarIn(),
                    exit = Motion.toolbarOut(),
                ) {
                    shownPanel[0]?.let {
                        when (it) {
                            Adjustment.HSB -> HsbPanel(vm, Modifier)
                            Adjustment.COLOR_BALANCE -> ColorBalancePanel(vm, Modifier)
                            Adjustment.CURVES -> CurvesPanel(vm, Modifier)
                            Adjustment.GRADIENT_MAP -> GradientMapPanel(vm, Modifier)
                            Adjustment.GLITCH -> GlitchPanel(vm, Modifier)
                            Adjustment.NOISE -> NoisePanel(vm, Modifier)
                            Adjustment.BLOOM -> BloomPanel(vm, Modifier)
                            Adjustment.HALFTONE -> HalftonePanel(vm, Modifier)
                            Adjustment.CHROMATIC_ABERRATION -> ChromaticPanel(vm, Modifier)
                            Adjustment.LIQUIFY -> LiquifyPanel(vm, maxWidth, maxHeight, Modifier)
                            else -> Unit
                        }
                    }
                }
                androidx.compose.animation.AnimatedVisibility(
                    vm.selecting && !vm.barsHidden,
                    Modifier.align(Alignment.BottomCenter),
                    enter = Motion.toolbarIn(),
                    exit = Motion.toolbarOut(),
                ) { SelectionToolbar(vm, maxWidth) }
                if (vm.selecting && !vm.barsHidden) {
                    SelectionPopover(vm, maxWidth, maxHeight)
                }
                GuideEditor(vm)
                LightingStudio(vm)
                BrushStudio(vm)
                PadMenu(vm)
            }
        }
    }
}

/**
 * The current layer turned a change away (locked, hidden, or an empty group): Cancel, unlock or
 * show it, or open Layers. What was tried is not done afterwards.
 */
@Composable
private fun BlockedDialog(vm: CanvasViewModel) {
    val last = androidx.compose.runtime.remember { arrayOfNulls<Pair<CanvasViewModel.Blocked, LayerInfo>>(1) }
    vm.blocked?.let { last[0] = it }
    val why = last[0]?.first
    Alert(
        vm.blocked,
        title = { (w, _) ->
            when (w) {
                CanvasViewModel.Blocked.LOCKED -> "Layer is locked"
                CanvasViewModel.Blocked.HIDDEN -> "Layer is hidden"
                CanvasViewModel.Blocked.EMPTY_GROUP -> "Group is empty"
            }
        },
        message = { (w, l) ->
            when (w) {
                CanvasViewModel.Blocked.LOCKED -> "“${l.name}” can't be changed while it is locked."
                CanvasViewModel.Blocked.HIDDEN -> "“${l.name}” can't be changed while it is hidden."
                CanvasViewModel.Blocked.EMPTY_GROUP -> "“${l.name}” holds no layers to change."
            }
        },
        action = "Open Layers",
        onCancel = { vm.answerBlocked(fix = false, openLayers = false) },
        onAction = { vm.answerBlocked(fix = false, openLayers = true) },
        icon = when (why) {
            CanvasViewModel.Blocked.LOCKED -> PcIcons.PageLocked
            CanvasViewModel.Blocked.HIDDEN -> PcIcons.EyeStruck
            else -> PcIcons.Folder
        },
        destructive = false,
        extra = when (why) {
            CanvasViewModel.Blocked.LOCKED -> "Unlock"
            CanvasViewModel.Blocked.HIDDEN -> "Show"
            else -> null
        },
        onExtra = { vm.answerBlocked(fix = true, openLayers = false) },
    )
}

/** The adjustments whose controls are a panel at the bottom of the screen. */
private val BottomPanels = setOf(
    Adjustment.HSB, Adjustment.COLOR_BALANCE, Adjustment.CURVES, Adjustment.GRADIENT_MAP, Adjustment.GLITCH, Adjustment.NOISE,
    Adjustment.BLOOM, Adjustment.HALFTONE, Adjustment.CHROMATIC_ABERRATION, Adjustment.LIQUIFY,
)

/**
 * What undo or redo did (and other passing messages), in a band across the screen just under the
 * top bar: it slides down from behind the bar, stays a moment and slides back up. A new message
 * takes the place of the one there: down, its text changes in place; moving, the band goes on
 * down from where it is. Either way it stays its full time from the new message.
 */
@Composable
private fun NoticeBand(vm: CanvasViewModel) {
    val shown = androidx.compose.runtime.remember { androidx.compose.animation.core.Animatable(0f) }
    var text by androidx.compose.runtime.remember { androidx.compose.runtime.mutableStateOf("") }
    val n = vm.notice
    androidx.compose.runtime.LaunchedEffect(n) {
        if (n == null) return@LaunchedEffect
        text = n.text
        val start = System.nanoTime()
        if (shown.value < 1f) shown.animateTo(1f, Motion.noticeIn())
        val shownFor = if (n.warning) Motion.NOTICE_WARNING_SHOWN_MS else Motion.NOTICE_SHOWN_MS
        kotlinx.coroutines.delay((shownFor - (System.nanoTime() - start) / 1_000_000).coerceAtLeast(0L))
        shown.animateTo(0f, Motion.noticeOut())
        vm.noticeGone(n)
    }
    if (shown.value <= 0f && n == null) return
    Box(Modifier.offset(y = Pc.BarHeight).fillMaxWidth().height(24.dp).clipToBounds()) {
        Box(
            Modifier.fillMaxSize().graphicsLayer { translationY = -(1f - shown.value) * size.height }.background(NoticeFill),
            contentAlignment = Alignment.Center,
        ) {
            Text(text, color = NoticeInk, fontSize = 13.sp, maxLines = 1)
        }
    }
}

private val NoticeFill = Color(0xC2424242)
private val NoticeInk = Color(0xFFD7D7D7)
