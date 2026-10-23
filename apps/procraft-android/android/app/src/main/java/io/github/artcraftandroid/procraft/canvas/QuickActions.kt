package io.github.artcraftandroid.procraft.canvas

/** What a QuickMenu button can be set to do. */
enum class QuickAction(val label: String) {
    NONE("No Action"),
    NEW_LAYER("New Layer"),
    DUPLICATE_LAYER("Duplicate Layer"),
    DELETE_LAYER("Delete Layer"),
    MERGE_DOWN("Merge Down"),
    CLEAR_LAYER("Clear Layer"),
    FILL_LAYER("Fill Layer"),
    SELECT_LAYER("Select Layer"),
    ALPHA_LOCK("Alpha Lock"),
    LAYER_VISIBILITY("Layer Visibility"),
    COPY("Copy"),
    CUT("Cut"),
    PASTE("Paste"),
    COPY_PASTE_MENU("Copy & Paste Menu"),
    FLIP_HORIZONTAL("Flip Horizontal"),
    FLIP_VERTICAL("Flip Vertical"),
    UNDO("Undo"),
    REDO("Redo"),
    PAINT("Paint"),
    SMUDGE("Smudge"),
    ERASE("Erase"),
    EYEDROPPER("Eyedropper"),
    SWAP_COLORS("Swap Colors"),
    SELECTION("Selection"),
    INVERT_SELECTION("Invert Selection"),
    TRANSFORM("Transform"),
    ADJUSTMENTS("Adjustments"),
    COLORS("Colors"),
    LAYERS("Layers"),
    BRUSHES("Brushes"),
    ACTIONS("Actions"),
    INTERFACE("Interface"),
    FIT_CANVAS("Fit Canvas"),
    ADD_TEXT("Add Text"),
    QUICKSHAPE("QuickShape"),
    LAYER_SELECT("Layer Select"),
    DRAWING_GUIDE("Drawing Guide"),
    ;

    /** What its button in the menu says: the name in sentence case, the flips as verbs. */
    val button: String
        get() = when (this) {
            FLIP_HORIZONTAL -> "Flip horizontally"
            FLIP_VERTICAL -> "Flip vertically"
            else -> label.take(1) + label.drop(1).lowercase()
        }

    companion object {
        /** The buttons a QuickMenu starts with, clockwise from the top. */
        val DEFAULTS = listOf(NEW_LAYER, FLIP_HORIZONTAL, COPY, MERGE_DOWN, CLEAR_LAYER, FLIP_VERTICAL)
    }
}
