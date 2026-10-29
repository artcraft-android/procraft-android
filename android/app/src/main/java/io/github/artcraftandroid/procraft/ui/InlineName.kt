package io.github.artcraftandroid.procraft.ui

import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.selection.LocalTextSelectionColors
import androidx.compose.foundation.text.selection.TextSelectionColors
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.TextUnit

/**
 * A name edited where it is shown: focused with all of it selected and the keyboard up; done on
 * the keyboard's return or when focus leaves, once. The selection is [color] at about a third, its
 * handles [color].
 */
@Composable
fun InlineName(name: String, color: Color, fontSize: TextUnit, modifier: Modifier = Modifier, onDone: (String) -> Unit) {
    var value by remember { mutableStateOf(TextFieldValue(name, TextRange(0, name.length))) }
    val focus = remember { FocusRequester() }
    var focused by remember { mutableStateOf(false) }
    var finished by remember { mutableStateOf(false) }
    val finish = {
        if (!finished) {
            finished = true
            onDone(value.text)
        }
    }
    LaunchedEffect(Unit) { focus.requestFocus() }
    val selection = TextSelectionColors(handleColor = color, backgroundColor = color.copy(alpha = 0.35f))
    CompositionLocalProvider(LocalTextSelectionColors provides selection) {
        BasicTextField(
            value,
            onValueChange = { value = it },
            modifier = modifier.focusRequester(focus).onFocusChanged {
                if (focused && !it.isFocused) finish()
                focused = it.isFocused
            },
            singleLine = true,
            textStyle = TextStyle(color = color, fontSize = fontSize),
            cursorBrush = SolidColor(color),
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
            keyboardActions = KeyboardActions(onDone = { finish() }),
        )
    }
}
