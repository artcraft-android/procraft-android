package io.github.artcraftandroid.procraft.canvas

import io.github.artcraftandroid.procraft.core.Adjustment
import io.github.artcraftandroid.procraft.core.QuickAction
import io.github.artcraftandroid.procraft.core.quickActionAdjustment
import io.github.artcraftandroid.procraft.core.quickActionLabel
import io.github.artcraftandroid.procraft.core.quickActionsListed
import io.github.artcraftandroid.procraft.core.quickAdjustmentsListed
import io.github.artcraftandroid.procraft.core.quickMenuSlots

// What a QuickMenu button can be set to: the actions, their names, the picker's order and how the
// buttons are kept are the core's (quick_menu.rs); what each does is the view model's.

private val labels: Map<QuickAction, String> by lazy { QuickAction.entries.associateWith { quickActionLabel(it) } }
private val defaults: List<QuickAction> by lazy { quickMenuSlots(null) }
private val listed: List<QuickAction?> by lazy { quickActionsListed() }
private val adjustmentsListed: List<QuickAction> by lazy { quickAdjustmentsListed() }

/** What its row in the picker says. */
val QuickAction.label: String get() = labels.getValue(this)

/** What its button in the menu says. */
val QuickAction.button: String get() = label

/** The adjustment it opens, if it is one of them. */
val QuickAction.adjustment: Adjustment? get() = quickActionAdjustment(this)

/** The buttons a QuickMenu starts with, clockwise from the top. */
val QuickAction.Companion.DEFAULTS: List<QuickAction> get() = defaults

/** The picker's rows: the actions by name (as written, capitals first), null for Adjustments. */
val QuickAction.Companion.LISTED: List<QuickAction?> get() = listed

/** The rows under Adjustments, by name. */
val QuickAction.Companion.ADJUSTMENTS_LISTED: List<QuickAction> get() = adjustmentsListed
