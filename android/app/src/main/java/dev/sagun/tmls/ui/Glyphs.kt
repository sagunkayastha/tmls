package dev.sagun.tmls.ui

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathFillType
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.PathBuilder
import androidx.compose.ui.graphics.vector.path
import androidx.compose.ui.unit.dp

/** The few icons the chat needs, drawn here as thin strokes: no icon library in the APK for a dozen glyphs. */
object Glyphs {
    private fun stroke(name: String, width: Float = 1.8f, draw: PathBuilder.() -> Unit): ImageVector =
        ImageVector.Builder(name, 24.dp, 24.dp, 24f, 24f).apply {
            path(
                fill = null, stroke = SolidColor(Color.White), strokeLineWidth = width,
                strokeLineCap = StrokeCap.Round, strokeLineJoin = StrokeJoin.Round, pathBuilder = draw,
            )
        }.build()

    private fun solid(name: String, draw: PathBuilder.() -> Unit): ImageVector =
        ImageVector.Builder(name, 24.dp, 24.dp, 24f, 24f).apply {
            path(fill = SolidColor(Color.White), pathFillType = PathFillType.NonZero, pathBuilder = draw)
        }.build()

    val Menu = stroke("menu") {
        moveTo(4f, 7f); lineTo(20f, 7f)
        moveTo(4f, 12f); lineTo(20f, 12f)
        moveTo(4f, 17f); lineTo(20f, 17f)
    }
    val More = solid("more") {
        for (y in listOf(5.5f, 12f, 18.5f)) {
            moveTo(12f, y - 1.6f)
            arcToRelative(1.6f, 1.6f, 0f, true, true, 0f, 3.2f)
            arcToRelative(1.6f, 1.6f, 0f, true, true, 0f, -3.2f)
            close()
        }
    }
    val Plus = stroke("plus") {
        moveTo(12f, 5f); lineTo(12f, 19f)
        moveTo(5f, 12f); lineTo(19f, 12f)
    }
    val Up = stroke("up", 2f) {
        moveTo(12f, 19f); lineTo(12f, 5f)
        moveTo(6f, 11f); lineTo(12f, 5f); lineTo(18f, 11f)
    }
    val Down = stroke("down") {
        moveTo(12f, 5f); lineTo(12f, 19f)
        moveTo(6f, 13f); lineTo(12f, 19f); lineTo(18f, 13f)
    }
    val ChevronRight = stroke("chevron-right") { moveTo(9f, 5f); lineTo(16f, 12f); lineTo(9f, 19f) }
    val ChevronDown = stroke("chevron-down") { moveTo(5f, 9f); lineTo(12f, 16f); lineTo(19f, 9f) }
    val Close = stroke("close") {
        moveTo(6f, 6f); lineTo(18f, 18f)
        moveTo(18f, 6f); lineTo(6f, 18f)
    }
    /** Two sheets, one behind the other. */
    val Copy = stroke("copy") {
        moveTo(9f, 9f); lineTo(20f, 9f); lineTo(20f, 20f); lineTo(9f, 20f); close()
        moveTo(15f, 9f); lineTo(15f, 4f); lineTo(4f, 4f); lineTo(4f, 15f); lineTo(9f, 15f)
    }
    /** Two arrows pulling apart: full screen. */
    val Expand = stroke("expand") {
        moveTo(14f, 4f); lineTo(20f, 4f); lineTo(20f, 10f)
        moveTo(20f, 4f); lineTo(13.5f, 10.5f)
        moveTo(10f, 20f); lineTo(4f, 20f); lineTo(4f, 14f)
        moveTo(4f, 20f); lineTo(10.5f, 13.5f)
    }
    val Stop = solid("stop") {
        moveTo(7f, 7f); lineTo(17f, 7f); lineTo(17f, 17f); lineTo(7f, 17f); close()
    }
    val Camera = stroke("camera") {
        moveTo(4f, 8f); lineTo(8f, 8f); lineTo(9.5f, 5.5f); lineTo(14.5f, 5.5f); lineTo(16f, 8f); lineTo(20f, 8f)
        lineTo(20f, 19f); lineTo(4f, 19f); close()
        moveTo(12f, 10f)
        arcToRelative(3.3f, 3.3f, 0f, true, true, 0f, 6.6f)
        arcToRelative(3.3f, 3.3f, 0f, true, true, 0f, -6.6f)
        close()
    }
    val Photo = stroke("photo") {
        moveTo(4f, 5f); lineTo(20f, 5f); lineTo(20f, 19f); lineTo(4f, 19f); close()
        moveTo(4f, 16f); lineTo(9f, 11f); lineTo(13f, 15f); lineTo(16f, 12f); lineTo(20f, 16f)
    }
    val File = stroke("file") {
        moveTo(7f, 3.5f); lineTo(14f, 3.5f); lineTo(18f, 7.5f); lineTo(18f, 20.5f); lineTo(7f, 20.5f); close()
        moveTo(14f, 3.5f); lineTo(14f, 7.5f); lineTo(18f, 7.5f)
    }
    val Mic = stroke("mic") {
        moveTo(9f, 6.5f)
        arcToRelative(3f, 3f, 0f, false, true, 6f, 0f)
        lineTo(15f, 11.5f)
        arcToRelative(3f, 3f, 0f, false, true, -6f, 0f)
        close()
        moveTo(5.5f, 11f)
        arcToRelative(6.5f, 6.5f, 0f, false, false, 13f, 0f)
        moveTo(12f, 17.5f); lineTo(12f, 20.5f)
    }
    /** A bookmark: kept for good. */
    val Keep = stroke("keep") {
        moveTo(7f, 4f); lineTo(17f, 4f); lineTo(17f, 20f); lineTo(12f, 16f); lineTo(7f, 20f); close()
    }
    val KeepOn = solid("keep-on") {
        moveTo(7f, 4f); lineTo(17f, 4f); lineTo(17f, 20f); lineTo(12f, 16f); lineTo(7f, 20f); close()
    }
}
