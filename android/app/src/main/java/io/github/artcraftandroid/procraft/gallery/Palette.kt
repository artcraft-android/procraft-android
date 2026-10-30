package io.github.artcraftandroid.procraft.gallery

/** Thirty swatches a palette holds: ten across, three down (the shared core keeps them). */
const val PALETTE_SLOTS = 30

/** A named set of swatches (0xRRGGBB, or null for an empty slot). */
data class Palette(val name: String, val colors: List<Int?>) {
    init {
        require(colors.size == PALETTE_SLOTS)
    }
}
