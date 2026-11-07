package io.github.artcraftandroid.procraft.canvas

import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class AskUntilTest {
    private val scope = CoroutineScope(SupervisorJob())

    @Test
    fun anAnswerComesBackAsSoonAsThereIsOne() = runBlocking {
        val asked = AtomicInteger()
        val answer = askUntil(scope, 5_000, 1) { if (asked.incrementAndGet() >= 3) "drawn" else null }
        assertEquals("drawn", answer)
        assertEquals(3, asked.get())
    }

    @Test
    fun aCallThatNeverComesBackCantHoldTheWaitPastItsLimit() = runBlocking {
        val letGo = CountDownLatch(1)
        val started = System.nanoTime()
        val answer = askUntil<String>(scope, 100, 1) {
            letGo.await()
            "too late"
        }
        val waited = TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - started)
        letGo.countDown()
        assertNull(answer)
        assertTrue("waited $waited ms", waited in 100..3_000)
    }

    @Test
    fun noAnswerByTheLimitIsNull() = runBlocking {
        assertNull(askUntil<String>(scope, 50, 1) { null })
    }

    @Test
    fun itAsksNoMoreOnceItsWaitIsOver() = runBlocking {
        val asked = AtomicInteger()
        askUntil<String>(scope, 30, 1) {
            asked.incrementAndGet()
            null
        }
        delay(20)
        val then = asked.get()
        delay(50)
        assertEquals(then, asked.get())
    }

    @Test
    fun aWaitCancelledFromOutsideAsksNoMoreEither() = runBlocking {
        val asked = AtomicInteger()
        val cancelled = runCatching {
            withTimeout(30) { askUntil<String>(scope, 60_000, 1) { asked.incrementAndGet(); null } }
        }
        assertTrue(cancelled.exceptionOrNull() is TimeoutCancellationException)
        delay(20)
        val then = asked.get()
        launch { delay(50) }.join()
        assertEquals(then, asked.get())
    }
}
