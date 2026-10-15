package dev.sagun.tmls.chat

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import java.time.ZoneId
import java.time.ZonedDateTime
import java.util.Locale

class UsageTest {
    private val now = 1_800_000_000L

    @Test fun theFeedsUsageMessage() {
        val msg = Wire.event(
            """{"t":"usage","usage":{"five_hour":{"pct":12,"resets_at":1800003600,"at":1799999000},
               "seven_day":{"pct":51,"resets_at":null,"at":1799999900},"at":1799999000}}"""
        ) as EventMsg.Usage
        assertEquals(
            Usage(UsageWindow(12, 1_800_003_600, 1_799_999_000), UsageWindow(51, null, 1_799_999_900), 1_799_999_000),
            msg.usage,
        )
    }

    @Test fun noUsageOrABrokenOneIsNull() {
        assertNull((Wire.event("""{"t":"usage","usage":null}""") as EventMsg.Usage).usage)
        assertNull((Wire.event("""{"t":"usage"}""") as EventMsg.Usage).usage)
        assertNull((Wire.event("""{"t":"usage","usage":{"five_hour":{"pct":"x"},"at":5}}""") as EventMsg.Usage).usage)
        assertNull((Wire.event("""{"t":"usage","usage":{"five_hour":{"pct":4}}}""") as EventMsg.Usage).usage)  // no at
    }

    @Test fun oneWindowAloneAndAWindowWithoutItsOwnTime() {
        val usage = (Wire.event("""{"t":"usage","usage":{"five_hour":null,"seven_day":{"pct":140.2},"at":7}}""") as EventMsg.Usage).usage!!
        assertNull(usage.fiveHour)
        assertEquals(UsageWindow(100, null, 7), usage.sevenDay)
    }

    @Test fun countdowns() {
        assertEquals("resets in 2h 14m", UsageText.countdown(now + 2 * 3600 + 14 * 60, now))
        assertEquals("resets in 14m", UsageText.countdown(now + 14 * 60, now))
        assertEquals("resets in 1m", UsageText.countdown(now + 5, now))  // never "0m" while ahead
        assertEquals("resets in 1d 3h", UsageText.countdown(now + 27 * 3600 + 120, now))
        assertEquals("resets in 5h 0m", UsageText.countdown(now + 5 * 3600, now))
        assertEquals("reset", UsageText.countdown(now, now))
        assertEquals("reset", UsageText.countdown(now - 60, now))
        assertEquals("reset", UsageText.countdown(null, now))
    }

    @Test fun theWeeklyResetIsADayAndALocalTime() {
        val zone = ZoneId.of("America/Denver")
        val friday9 = ZonedDateTime.of(2026, 10, 16, 9, 0, 0, 0, zone).toEpochSecond()
        val friday21 = ZonedDateTime.of(2026, 10, 16, 21, 5, 0, 0, zone).toEpochSecond()
        val before = friday9 - 86400
        assertEquals("resets Fri 9:00", UsageText.resetsOn(friday9, before, zone, h24 = true, locale = Locale.US))
        assertEquals("resets Fri 21:05", UsageText.resetsOn(friday21, before, zone, h24 = true, locale = Locale.US))
        assertEquals("resets Fri 9:05 PM", UsageText.resetsOn(friday21, before, zone, h24 = false, locale = Locale.US))
        assertEquals("reset", UsageText.resetsOn(friday9, friday9 + 1, zone, h24 = true, locale = Locale.US))
        assertEquals("reset", UsageText.resetsOn(null, before, zone, h24 = true, locale = Locale.US))
    }

    @Test fun colorsAsItFills() {
        assertEquals(Fill.NORMAL, UsageText.fill(0))
        assertEquals(Fill.NORMAL, UsageText.fill(74))
        assertEquals(Fill.HIGH, UsageText.fill(75))
        assertEquals(Fill.HIGH, UsageText.fill(89))
        assertEquals(Fill.FULL, UsageText.fill(90))
        assertEquals(Fill.FULL, UsageText.fill(100))
    }

    @Test fun howOldOnlyOnceItMatters() {
        assertNull(UsageText.asOf(now - 9 * 60, now))
        assertEquals("as of 10 min ago", UsageText.asOf(now - 10 * 60, now))
        assertEquals("as of 59 min ago", UsageText.asOf(now - 3599, now))
        assertEquals("as of 3 h ago", UsageText.asOf(now - 3 * 3600 - 5, now))
        assertEquals("as of 4 days ago", UsageText.asOf(now - 4 * 86400, now))
    }

    @Test fun contextIsLabelled() {
        assertEquals("ctx 14%", UsageText.context(14))
    }
}
