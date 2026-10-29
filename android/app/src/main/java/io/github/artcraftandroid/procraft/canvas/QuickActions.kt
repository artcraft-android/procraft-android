package io.github.artcraftandroid.procraft.canvas

/**
 * What a QuickMenu button can be set to, by the name its picker lists. The adjustments are listed
 * under Adjustments ([adjustment] says which). The names saved are the entries' names, so an
 * entry keeps its name when its label changes.
 */
enum class QuickAction(val label: String, val adjustment: Adjustment? = null, val ready: Boolean = true) {
    NONE("No Action"),
    GUIDE_2D("2D Guide visibility"),
    ACTUAL_SIZE("Actual size"),
    ADD_TEXT("Add Text"),
    ALPHA_LOCK("Alpha Lock"),
    ANIMATION_ASSIST("Animation Assist"),
    BRUSH_SCALING("Brush scaling"),
    CLEAR_LAYER("Clear layer"),
    COPY_PASTE_MENU("Clipboard"),
    CLONE("Clone"),
    COPY("Copy"),
    COPY_CANVAS("Copy canvas"),
    CUT("Cut"),
    DELETE_LAYER("Delete layer"),
    DRAWING_ASSIST("Drawing Assist"),
    DUPLICATE_LAYER("Duplicate layer"),
    ERASE("Erase"),
    EYEDROPPER("Eyedropper"),
    FILL_LAYER("Fill layer"),
    FIT_CANVAS("Fit canvas"),
    FLIP_HORIZONTAL("Flip horizontally"),
    FLIP_VERTICAL("Flip vertically"),
    INTERFACE("Full screen"),
    GUIDE_ISOMETRIC("Isometric Guide visibility"),
    LAYER_SELECT("Layer Select"),
    LAYER_OPACITY("Layer opacity"),
    LAYER_VISIBILITY("Layer visibility"),
    LIQUIFY("Liquify"),
    MERGE_DOWN("Merge down"),
    NEW_LAYER("New layer"),
    NEXT_FRAME("Next frame"),
    ONION_SKIN("Onion skin"),
    ADJUSTMENTS("Open Adjustments"),
    COLORS("Open Color panel"),
    LAYERS("Open Layers panel"),
    PAGE_ASSIST("Page Assist"),
    PAINT("Paint"),
    PASTE("Paste"),
    GUIDE_PERSPECTIVE("Perspective Guide visibility"),
    PREVIOUS_BRUSH("Previous brush"),
    SWAP_COLORS("Previous color"),
    PREVIOUS_FRAME("Previous frame"),
    PROJECT_CANVAS("Project canvas"),
    QUICKSHAPE_ON("QuickShape"),
    QUICKSHAPE("QuickShape stroke"),
    // not built yet: left out of the picker
    RECOLOR("Recolor"),
    REDO("Redo"),
    REFERENCE("Reference"),
    BRUSHES("Select brush"),
    SELECT_LAYER("Select layer contents"),
    SELECTION("Selection tool"),
    COMPANION_MOVE("Single touch move"),
    COMPANION_ZOOM("Single touch zoom"),
    SMUDGE("Smudge"),
    SOLO("Solo visibility"),
    GUIDE_SYMMETRY("Symmetry Guide visibility"),
    TRANSFORM("Transform tool"),
    UNDO("Undo"),

    // under Adjustments
    BLOOM("Bloom", Adjustment.BLOOM),
    CHROMATIC_ABERRATION("Chromatic Aberration", Adjustment.CHROMATIC_ABERRATION),
    COLOR_BALANCE("Color Balance", Adjustment.COLOR_BALANCE),
    CURVES("Curves", Adjustment.CURVES),
    GAUSSIAN_BLUR("Gaussian Blur", Adjustment.GAUSSIAN_BLUR),
    GLITCH("Glitch", Adjustment.GLITCH),
    GRADIENT_MAP("Gradient Map", Adjustment.GRADIENT_MAP),
    HALFTONE("Halftone", Adjustment.HALFTONE),
    HSB("Hue, Saturation, Brightness", Adjustment.HSB),
    MOTION_BLUR("Motion Blur", Adjustment.MOTION_BLUR),
    NOISE("Noise", Adjustment.NOISE),
    PERSPECTIVE_BLUR("Perspective Blur", Adjustment.PERSPECTIVE_BLUR),
    SHARPEN("Sharpen", Adjustment.SHARPEN),
    ;

    /** What its button in the menu says. */
    val button: String get() = label

    companion object {
        /** The buttons a QuickMenu starts with, clockwise from the top. */
        val DEFAULTS = listOf(NEW_LAYER, FLIP_HORIZONTAL, COPY, MERGE_DOWN, CLEAR_LAYER, FLIP_VERTICAL)

        /** The picker's rows: the actions by name (as written, capitals first), null for Adjustments. */
        val LISTED: List<QuickAction?> = (entries.filter { it.adjustment == null && it.ready }.map { it.label to it } + ("Adjustments" to null))
            .sortedBy { it.first }.map { it.second }

        /** The rows under Adjustments, by name. */
        val ADJUSTMENTS_LISTED: List<QuickAction> = entries.filter { it.adjustment != null }.sortedBy { it.label }
    }
}
