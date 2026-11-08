package io.github.artcraftandroid.procraft.canvas

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.delay
import kotlinx.coroutines.withTimeoutOrNull

/**
 * Asks [ask] every [pollMs] until it has an answer, for at most [limitMs] by the clock; null if it
 * had none by then. [ask] may block: it runs apart in [scope], so a call that doesn't come back
 * can't hold the wait past its limit (it is left to end by itself, and asks no more).
 */
internal suspend fun <T : Any> askUntil(scope: CoroutineScope, limitMs: Long, pollMs: Long, ask: () -> T?): T? {
    val asking = scope.async(Dispatchers.IO) {
        var answer = ask()
        while (answer == null) {
            delay(pollMs)
            answer = ask()
        }
        answer
    }
    return try {
        withTimeoutOrNull(limitMs) { asking.await() }
    } finally {
        asking.cancel()
    }
}
