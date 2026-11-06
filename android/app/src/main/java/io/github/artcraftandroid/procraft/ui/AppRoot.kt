package io.github.artcraftandroid.procraft.ui

import androidx.compose.foundation.draganddrop.dragAndDropTarget
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
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.displayCutout
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
import androidx.compose.ui.zIndex
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.ui.viewinterop.AndroidView
import io.github.artcraftandroid.procraft.core.Adjustment
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
    val interfaceSize = fittedInterfaceSize(vm.plugins, vm.pluginList)
    androidx.compose.runtime.SideEffect { vm.interfaceShownAt(interfaceSize) }
    ProCraftTheme(interfaceSize) {
        if (vm.screen == Screen.GALLERY) {
            ZoomingGallery(vm, overlay = false)
            // a share from the gallery shows its card there, and a file brought in its own
            ExportProgress(vm.exportShown, vm::cancelExport)
            ImportProgress(vm.importShown, vm::cancelImport)
            PluginOfferAlert(vm)
            return@ProCraftTheme
        }
        // back closes an open panel first, then leaves the artwork for the gallery
        BackHandler {
            when {
                vm.guideEditing -> vm.closeGuideEditor(keep = false)
                vm.cropRect != null -> vm.endCrop(false)
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
        val window = androidx.compose.ui.platform.LocalView.current.rootView
        androidx.compose.runtime.DisposableEffect(window) {
            vm.backdrops.windowOnScreen = {
                val at = IntArray(2).also(window::getLocationOnScreen)
                androidx.compose.ui.geometry.Offset(at[0].toFloat(), at[1].toFloat())
            }
            onDispose { vm.backdrops.windowOnScreen = { androidx.compose.ui.geometry.Offset.Zero } }
        }
        CompositionLocalProvider(LocalBackdrops provides vm.backdrops) {
            // files dragged in from another app: pictures as layers, brushes into the library
            val activity = androidx.compose.ui.platform.LocalContext.current.activity()
            val dropped = androidx.compose.runtime.remember(vm) {
                object : androidx.compose.ui.draganddrop.DragAndDropTarget {
                    override fun onDrop(event: androidx.compose.ui.draganddrop.DragAndDropEvent): Boolean {
                        val uris = event.droppedFiles(activity)
                        vm.dropOnCanvas(uris)
                        return uris.isNotEmpty()
                    }
                }
            }
            BoxWithConstraints(
                Modifier.fillMaxSize().background(Pc.Surround).dragAndDropTarget(
                    shouldStartDragAndDrop = { it.fromElsewhere() },
                    target = dropped,
                ),
            ) {
                // the bars keep clear of the screen's cutout, and the top bar takes two rows where one is too narrow
                val density = androidx.compose.ui.platform.LocalDensity.current
                val cutout = WindowInsets.displayCutout
                val direction = androidx.compose.ui.platform.LocalLayoutDirection.current
                with(density) {
                    Pc.safeTop = cutout.getTop(this).toDp()
                    Pc.safeLeft = cutout.getLeft(this, direction).toDp()
                    Pc.safeRight = cutout.getRight(this, direction).toDp()
                }
                Pc.barRows = Pc.barFit(maxWidth - Pc.safeLeft - Pc.safeRight).rows
                // a phone's window gets ProCraft Pocket; held sideways, its card goes in two pills across the top
                Pc.pocket = minOf(maxWidth, maxHeight) < Pc.PocketBelow
                Pc.pocketSplit = Pc.pocket && maxWidth - Pc.safeLeft - Pc.safeRight >= Pc.PocketSplitFrom
                val barHeight = Pc.BarHeight
                androidx.compose.runtime.LaunchedEffect(barHeight) { vm.applyFitInsets() }
                AndroidView(factory = { CanvasSurfaceView(it, vm) }, modifier = Modifier.fillMaxSize())
                // the drawing guide is the canvas's: under every bar and panel
                GuideOverlay(vm)
                if (vm.transforming) TransformOverlay(vm)
                if (vm.selecting) SelectionOverlay(vm, Pc.BarHeight)
                vm.picking?.let { pick ->
                    val pickingZ = if (pick.fromReference) 1f else 0f
                    Loupe(pick, CanvasSpace.color(vm.colorRgb()), Modifier.zIndex(pickingZ))
                    if (!vm.barsHidden) PickingBar(Modifier.offset(y = Pc.BarHeight).zIndex(pickingZ), pick.fromReference)
                }
                // the bars fade out as the interface hides, and back
                val barsShown = androidx.compose.animation.core.animateFloatAsState(if (vm.barsHidden) 0f else 1f, Motion.bars(), label = "bars").value
                // with the interface hidden, a button in the top left brings it back
                if (barsShown < 1f) {
                    Box(
                        Modifier.offset(x = Pc.safeLeft + 20.dp, y = Pc.safeTop).size(44.dp).graphicsLayer { alpha = 1f - barsShown }
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
                    val height = with(androidx.compose.ui.platform.LocalDensity.current) { Pc.BarHeight.toPx() }
                    val topGlass = BarGlass(barsShown, androidx.compose.ui.geometry.Offset(0f, -height * (1f - barsShown)))
                    if (Pc.pocket) {
                        PocketBar(vm, Modifier.align(Alignment.TopStart), barsShown)
                    } else {
                        TopBar(vm, Modifier.align(Alignment.TopCenter).then(rememberFrost(0.dp, topGlass)).then(up).then(fade))
                    }
                    val sideBar = Pc.sideBarFit(maxHeight)
                    val sideInset = if (Pc.pocket) Pc.PocketInset else 0.dp
                    // a mode taking the screen slides it off its edge, and back when it ends
                    androidx.compose.animation.AnimatedVisibility(
                        vm.showSideBar && !vm.transforming && !vm.selecting && !vm.guideEditing && !vm.lightingStudioOpen && vm.cropRect == null && vm.textInfo == null &&
                            vm.opacityLayer == null &&
                            (vm.adjustment == null || vm.adjustPencil) &&
                            // ProCraft Pocket's sheets rise over where it floats: it steps aside while one is open
                            !(Pc.pocket && vm.panel != Panel.NONE),
                        Modifier.align(if (vm.rightHanded) Alignment.TopEnd else Alignment.TopStart)
                            .offset(x = if (vm.rightHanded) -(Pc.safeRight + sideInset) else Pc.safeLeft + sideInset, y = sideBar.top),
                        enter = Motion.sideBarIn(vm.rightHanded),
                        exit = Motion.sideBarOut(vm.rightHanded),
                    ) {
                        SideBar(vm, aside.then(fade), right = vm.rightHanded, trackHeight = sideBar.track, shown = barsShown, floating = Pc.pocket)
                    }
                }
                NoticeBand(vm)
                DropBandUi(vm)
                // a full-screen page covers the timeline, as it covers the canvas; the panels open over it
                val pageOver = vm.canvasInfoOpen || vm.gestureControlsOpen || vm.cropRect != null || vm.timeLapseReplaying ||
                    vm.animatedExport != null || vm.modelAnimatedExport != null
                if (!vm.barsHidden && !pageOver) AnimationBar(vm)
                // the bottom toolbars rise from below the screen, and drop away
                androidx.compose.animation.AnimatedVisibility(
                    vm.transforming && !vm.barsHidden,
                    Modifier.align(Alignment.BottomCenter),
                    enter = Motion.toolbarIn(),
                    exit = Motion.toolbarOut(),
                ) { TransformToolbar(vm, maxWidth) }
                BrushCursor(vm)
                // the Reference window floats over the bars and the canvas's own marks; the panels,
                // menus and pages opened later come over it
                ReferenceWindow(vm)
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
                    // in ProCraft Pocket a panel is a sheet rising from the bottom, in the middle
                    val sheet = Pc.pocket
                    val gap = if (sheet) Pc.PocketSheetGap else Pc.PopoverGap
                    // on a screen narrower than the panel (a phone), all of it shrinks to fit
                    val fit = if (width > 0.dp) ((maxWidth - Pc.safeLeft - Pc.safeRight - gap * 2) / width).coerceIn(0.1f, 1f) else 1f
                    val place: PopoverPlace
                    val pivotAt: androidx.compose.ui.geometry.Offset
                    val origin01: androidx.compose.ui.graphics.TransformOrigin
                    if (sheet) {
                        val left = Pc.safeLeft + (maxWidth - Pc.safeLeft - Pc.safeRight - width) / 2
                        place = PopoverPlace(left, width / 2)
                        pivotAt = with(density) { androidx.compose.ui.geometry.Offset((left + width / 2).toPx(), (maxHeight - gap).toPx()) }
                        origin01 = androidx.compose.ui.graphics.TransformOrigin(0.5f, 1f)
                    } else {
                        val fromLeft = shown == Panel.ACTIONS || shown == Panel.ADJUSTMENTS
                        val buttonX = if (fromLeft) Pc.actionCentre(button, maxWidth) else maxWidth - Pc.toolCentreFromRight(button, maxWidth)
                        // panels keep clear of the side bar, on whichever side it is, and of the cutout
                        val bar = if (vm.showSideBar) Pc.SideBarWidth else 0.dp
                        val minLeft = Pc.safeLeft + Pc.PopoverGap + if (vm.rightHanded) 0.dp else bar
                        val minRight = Pc.safeRight + Pc.PopoverGap + if (vm.rightHanded) bar else 0.dp
                        val fitted = popoverPlace(width * fit, buttonX, maxWidth, minLeft, minRight)
                        // the arrow stays on the panel's straight edge, clear of its rounded corners
                        val reach = Pc.PopoverCorner + PanelArrowClear
                        val arrowX = (fitted.arrowX / fit).coerceIn(reach, (width - reach).coerceAtLeast(reach))
                        // shrinking about its arrow, its body lands where [fitted] put it
                        val left = fitted.left - arrowX * (1f - fit)
                        place = PopoverPlace(left, arrowX)
                        pivotAt = with(density) { androidx.compose.ui.geometry.Offset((left + arrowX).toPx(), Pc.BarHeight.toPx()) }
                        origin01 = androidx.compose.ui.graphics.TransformOrigin(if (width > 0.dp) arrowX / width else 0f, 0f)
                    }
                    val pivot = origin + pivotAt
                    val look = Modifier.graphicsLayer {
                        this.alpha = alpha
                        scaleX = scale * fit
                        scaleY = scale * fit
                        transformOrigin = origin01
                    }.then(if (vm.panel == Panel.NONE) Modifier.takingNoTouches() else Modifier)
                    val at = if (sheet) Modifier.offset(x = place.left).then(look) else Modifier.offset(x = place.left, y = Pc.BarHeight).then(look)
                    // shrunk, a panel can be as much taller as it shows shorter; a sheet reaches up to under the bar
                    fun tall(rest: Dp) = if (sheet) (maxHeight - Pc.BarHeight - gap * 2) / fit else (maxHeight - Pc.BarHeight - rest) / fit
                    // the brush library, as tall as it can be where it hangs, is a sheet over the lower part of the screen
                    val libraryHeight = if (sheet) minOf(tall(25.dp), maxHeight * PocketLibraryShare / fit) else tall(25.dp)
                    Box(
                        if (sheet) Modifier.fillMaxSize().padding(bottom = gap) else Modifier,
                        contentAlignment = Alignment.BottomStart,
                    ) {
                        ProvidePanelLook(PanelLook(alpha, scale * fit, pivot)) {
                            // the colour panel's grabber is its own: pulled, it floats over the canvas
                            val sheetLook = if (!sheet) null else PopoverSheet(if (shown == Panel.COLOR || (shown == Panel.LAYERS && vm.backgroundPicking)) null else ({ vm.closePanels() }))
                            CompositionLocalProvider(LocalPopoverSheet provides sheetLook) {
                                when (shown) {
                                    Panel.LAYERS -> if (vm.backgroundPicking) {
                                        ColorPanel(vm, place.arrowX, Pc.BarHeight + 25.dp + tall(25.dp), at, background = true)
                                        // closed any way at all, the colour is kept
                                        androidx.compose.runtime.DisposableEffect(Unit) { onDispose { vm.endBackgroundPick() } }
                                    } else {
                                        LayersPanel(vm, place.arrowX, at.heightIn(max = tall(Pc.PopoverGap)))
                                    }
                                    Panel.COLOR -> ColorPanel(vm, place.arrowX, Pc.BarHeight + 25.dp + tall(25.dp), at)
                                    Panel.BRUSHES -> BrushLibrary(vm, place.arrowX, at.height(libraryHeight))
                                    // ending 25 above the screen's edge, their rows scrolling (as measured)
                                    Panel.ACTIONS -> ActionsPanel(vm, place.arrowX, at.heightIn(max = tall(25.dp)))
                                    Panel.ADJUSTMENTS -> AdjustmentsPanel(vm, place.arrowX, at.heightIn(max = tall(25.dp)))
                                    Panel.NONE -> Unit
                                }
                            }
                        }
                    }
                }
                // the colour panel dragged out by its grabber: a window over the canvas until closed
                FloatingColors(vm, maxWidth, maxHeight)
                if (vm.transforming && !vm.barsHidden) {
                    TransformPopover(vm, maxWidth, maxHeight)
                    TransformNumeric(vm)
                }
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
                ModelPicture(vm)
                BlockedDialog(vm)
                ModelWarningsAlert(vm)
                SharedLayerAlert(vm)
                PluginOfferAlert(vm)
                // a fill a plugin is finishing: nothing is touched until it is done
                if (vm.fillFinishing) {
                    Box(
                        Modifier.fillMaxSize().pointerInput(Unit) {
                            awaitPointerEventScope { while (true) awaitPointerEvent().changes.forEach { it.consume() } }
                        },
                    )
                }
                ExportProgress(vm.exportShown, vm::cancelExport)
                ImportProgress(vm.importShown, vm::cancelImport)
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
                if (vm.adjustment == Adjustment.CLONE) ClonePuck(vm)
                if (vm.adjustment == Adjustment.PERSPECTIVE_BLUR) PerspectivePuck(vm)
                if (vm.recoloring) RecolorReticle(vm)
                QuickShapeHandles(vm)
                CopyPastePanel(vm)
                QuickMenu(vm)
                LayerChoice(vm)
                if (!vm.barsHidden) QuickShapeBar(vm, Modifier.offset(y = Pc.BarHeight))
                if (vm.opacityLayer != null && !vm.barsHidden) LayerOpacityBar(vm, Modifier.offset(y = Pc.BarHeight))
                if (vm.adjustment != null && !vm.barsHidden) {
                    AdjustmentBar(vm, Modifier.offset(y = Pc.BarHeight))
                    AdjustModes(vm, Modifier.align(Alignment.TopCenter).offset(y = Pc.BarHeight + 52.dp))
                    AdjustmentMenu(vm, maxWidth, maxHeight)
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
                            Adjustment.PERSPECTIVE_BLUR -> PerspectivePanel(vm, Modifier)
                            Adjustment.LIQUIFY -> LiquifyPanel(vm, maxWidth, maxHeight, Modifier)
                            Adjustment.PLUGIN -> PluginFilterPanel(vm, Modifier)
                            else -> Unit
                        }
                    }
                }
                androidx.compose.animation.AnimatedVisibility(
                    vm.recoloring && !vm.barsHidden,
                    Modifier.align(Alignment.BottomCenter),
                    enter = Motion.toolbarIn(),
                    exit = Motion.toolbarOut(),
                ) { RecolorPanel(vm, Modifier) }
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
                PadMenu(vm, maxWidth)
                // a colour dragged out goes over everything
                DropDot(vm)
            }
        }
    }
}

/**
 * The current layer turned a change away (locked, hidden or left out of the picture, or an empty
 * group): Cancel, unlock or show it, or open Layers. What was tried is not done afterwards.
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
                CanvasViewModel.Blocked.MESH_HIDDEN -> "Mesh is hidden"
                CanvasViewModel.Blocked.MESH_LOCKED -> "Mesh is locked"
                CanvasViewModel.Blocked.GROUP_HIDDEN -> "Group is hidden"
                CanvasViewModel.Blocked.GROUP -> "Group is selected"
            }
        },
        message = { (w, l) ->
            when (w) {
                CanvasViewModel.Blocked.LOCKED -> "“${l.name}” can't be changed while it is locked."
                CanvasViewModel.Blocked.HIDDEN, CanvasViewModel.Blocked.GROUP_HIDDEN -> hiddenMessage(l.name, vm.blockedBy)
                CanvasViewModel.Blocked.EMPTY_GROUP -> "“${l.name}” holds no layers to change."
                CanvasViewModel.Blocked.GROUP -> "“${l.name}” is a group: choose one of its layers to paint on."
                CanvasViewModel.Blocked.MESH_HIDDEN -> "“${vm.meantMeshName()}” can't be painted while it is hidden."
                CanvasViewModel.Blocked.MESH_LOCKED -> "“${vm.meantMeshName()}” can't be painted while it is locked."
            }
        },
        action = "Open Layers",
        onCancel = { vm.answerBlocked(fix = false, openLayers = false) },
        onAction = { vm.answerBlocked(fix = false, openLayers = true) },
        icon = when (why) {
            CanvasViewModel.Blocked.LOCKED, CanvasViewModel.Blocked.MESH_LOCKED -> PcIcons.PageLocked
            CanvasViewModel.Blocked.HIDDEN, CanvasViewModel.Blocked.MESH_HIDDEN, CanvasViewModel.Blocked.GROUP_HIDDEN -> PcIcons.EyeStruck
            else -> PcIcons.Folder
        },
        destructive = false,
        extra = when (why) {
            CanvasViewModel.Blocked.LOCKED, CanvasViewModel.Blocked.MESH_LOCKED -> "Unlock"
            CanvasViewModel.Blocked.HIDDEN, CanvasViewModel.Blocked.MESH_HIDDEN, CanvasViewModel.Blocked.GROUP_HIDDEN -> "Show"
            else -> null
        },
        onExtra = { vm.answerBlocked(fix = true, openLayers = false) },
    )
}

/** Why layer or group [name], which would hide a change, takes none, by what hides it ([by]). */
private fun hiddenMessage(name: String, by: io.github.artcraftandroid.procraft.core.HiddenBy?): String =
    "“$name” can't be changed while " + when (by) {
        io.github.artcraftandroid.procraft.core.HiddenBy.MASK_LAYER -> "its layer is hidden."
        io.github.artcraftandroid.procraft.core.HiddenBy.GROUP -> "its group is hidden."
        io.github.artcraftandroid.procraft.core.HiddenBy.INSIDE -> "a layer in it is hidden."
        io.github.artcraftandroid.procraft.core.HiddenBy.SOLO -> "another layer is shown alone."
        io.github.artcraftandroid.procraft.core.HiddenBy.ITSELF, null -> "it is hidden."
    }

/** A layer a texture set's meshes share is about to go from all of them. */
@Composable
internal fun SharedLayerAlert(vm: CanvasViewModel) {
    Alert(
        vm.deletingShared,
        title = { "Delete shared layer?" },
        message = { "“${it.name}” is on every mesh of this texture set." },
        action = "Delete",
        onCancel = { vm.deletingShared = null },
        onAction = {
            vm.deletingShared = null
            vm.delete(it)
        },
    )
}

/** What a model newly opened may not paint well with, said once. */
@Composable
internal fun ModelWarningsAlert(vm: CanvasViewModel) {
    Alert(
        vm.modelWarnings.takeIf { it.isNotEmpty() },
        title = { "Unsupported model features" },
        message = { w -> "Painting on this model may not go as expected because of:\n" + w.joinToString("\n") },
        action = "OK",
        onCancel = { vm.modelWarnings = emptyList() },
        onAction = { vm.modelWarnings = emptyList() },
        destructive = false,
        alone = true,
    )
}

/** How far past its rounded corner a panel's arrow stays, so that it sits on the straight edge. */
private val PanelArrowClear = 6.dp

/** How much of the screen's height ProCraft Pocket's brush library takes, rising from the bottom. */
private const val PocketLibraryShare = 0.6f

/** The adjustments whose controls are a panel at the bottom of the screen. */
private val BottomPanels = setOf(
    Adjustment.HSB, Adjustment.COLOR_BALANCE, Adjustment.CURVES, Adjustment.GRADIENT_MAP, Adjustment.GLITCH, Adjustment.NOISE,
    Adjustment.BLOOM, Adjustment.HALFTONE, Adjustment.CHROMATIC_ABERRATION, Adjustment.PERSPECTIVE_BLUR, Adjustment.LIQUIFY,
    Adjustment.PLUGIN,
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
        // frosted over the canvas, as measured (a thin line under it doesn't show through); moved
        // as it is placed, so the frost follows it
        Box(
            Modifier.fillMaxSize()
                .offset { androidx.compose.ui.unit.IntOffset(0, kotlin.math.round(-(1f - shown.value) * 24.dp.toPx()).toInt()) }
                .then(rememberFrost(0.dp)).background(NoticeFill),
            contentAlignment = Alignment.Center,
        ) {
            Text(text, color = NoticeInk, fontSize = 13.sp, maxLines = 1)
        }
    }
}

private val NoticeFill = Color(0xC2424242)
private val NoticeInk = Color(0xFFD7D7D7)
