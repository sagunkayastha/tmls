package dev.sagun.tmls.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

/** The chat's colours: dark only, a warm near-black, one terracotta accent. */
object Tm {
    val Bg = Color(0xFF1A1917)
    /** Things that sit on the background: the input bar, the permission card, the drawer's selection. */
    val Raised = Color(0xFF262522)
    /** Things sunk into it: the user's bubbles, code boxes. */
    val Sunken = Color(0xFF100F0E)
    val Text = Color(0xFFECE8E1)
    val Dim = Color(0xFF9C978E)
    val Faint = Color(0xFF6B6760)
    val Line = Color(0xFF35332F)
    val Accent = Color(0xFFC6613F)
    /** Inline code and links. */
    val Ink = Color(0xFF7DB1E8)

    // The web page's status colours (style.css: --run, --wait, --accent, --fail, --dim).
    private val Run = Color(0xFF4EBF71)
    private val Wait = Color(0xFFE5C07B)
    private val Done = Color(0xFFFF8C00)
    val Fail = Color(0xFFE06C75)
    private val Idle = Color(0xFF8A929C)

    fun mark(mark: String): Color = when (mark) {
        "running" -> Run
        "waiting" -> Wait
        "done" -> Done
        "failed" -> Fail
        else -> Idle
    }

    /** A usage window's or a context's colour: [normal] below 75 %, amber, then red from 90 %. */
    fun fill(fill: dev.sagun.tmls.chat.Fill, normal: Color): Color = when (fill) {
        dev.sagun.tmls.chat.Fill.NORMAL -> normal
        dev.sagun.tmls.chat.Fill.HIGH -> Wait
        dev.sagun.tmls.chat.Fill.FULL -> Fail
    }

    /** Body text: large, with room between the lines. The platform's sans; its monospace for code. */
    val Body = TextStyle(fontSize = 17.sp, lineHeight = 26.sp, color = Text)
    val Small = TextStyle(fontSize = 14.sp, lineHeight = 20.sp, color = Dim)
    val Mono = TextStyle(fontFamily = FontFamily.Monospace, fontSize = 14.sp, lineHeight = 20.sp, color = Text)
}

@Composable
fun TmlsTheme(content: @Composable () -> Unit) {
    val base = Typography()
    MaterialTheme(
        colorScheme = darkColorScheme(
            primary = Tm.Accent,
            onPrimary = Color.White,
            primaryContainer = Tm.Accent,
            onPrimaryContainer = Color.White,
            secondary = Tm.Accent,
            background = Tm.Bg,
            onBackground = Tm.Text,
            surface = Tm.Bg,
            onSurface = Tm.Text,
            surfaceVariant = Tm.Raised,
            onSurfaceVariant = Tm.Dim,
            surfaceContainerLowest = Tm.Sunken,
            surfaceContainerLow = Tm.Bg,
            surfaceContainer = Tm.Raised,
            surfaceContainerHigh = Tm.Raised,
            surfaceContainerHighest = Tm.Raised,
            outline = Tm.Line,
            outlineVariant = Tm.Line,
            error = Tm.Fail,
        ),
        typography = base.copy(
            bodyLarge = Tm.Body,
            bodyMedium = base.bodyMedium.copy(fontSize = 15.sp, lineHeight = 22.sp),
            titleMedium = base.titleMedium.copy(fontSize = 17.sp, fontWeight = FontWeight.SemiBold),
        ),
        content = content,
    )
}
