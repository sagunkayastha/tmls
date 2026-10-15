package dev.sagun.tmls.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import dev.sagun.tmls.chat.Align
import dev.sagun.tmls.chat.Block
import dev.sagun.tmls.chat.ImagePaths
import dev.sagun.tmls.chat.Markdown
import dev.sagun.tmls.chat.Rich
import dev.sagun.tmls.chat.Style

/** What the blocks of a reply can ask of the screen. */
class MarkdownActions(
    val openLink: (String) -> Unit,
    val copy: (String) -> Unit,
    /** A code box, full screen: its label and its text. */
    val showCode: (String, String) -> Unit,
    /** Thumbnails of the picture files these paths name (on the session's host), under the text naming them. */
    val pictures: @Composable (List<String>) -> Unit = {},
)

/** A reply: its blocks down the page, on the background, with no box around them. */
@Composable
fun MarkdownView(blocks: List<Block>, actions: MarkdownActions, modifier: Modifier = Modifier) {
    Column(modifier, verticalArrangement = Arrangement.spacedBy(12.dp)) {
        blocks.forEach { BlockView(it, actions) }
    }
}

/** The picture paths a block names, its nested blocks too; code is code, not a picture. */
private fun paths(block: Block): List<String> = when (block) {
    is Block.Para -> ImagePaths.find(block.text.text)
    is Block.Head -> ImagePaths.find(block.text.text)
    is Block.Items -> block.items.flatten().flatMap(::paths)
    is Block.Quote -> block.blocks.flatMap(::paths)
    is Block.Table -> (block.head + block.rows.flatten()).flatMap { ImagePaths.find(it.text) }
    is Block.CodeBox, Block.Rule -> emptyList()
}.distinct()

private val HEADINGS = listOf(24.sp to 32.sp, 21.sp to 29.sp, 19.sp to 27.sp)

/**
 * One block. [pictures]: draw thumbnails for the picture paths it names. A paragraph has its own
 * under it; a list (or table) naming several has one grid under the whole of it instead, so a
 * list of paths becomes a tidy grid rather than a column of single pictures.
 */
@Composable
private fun BlockView(block: Block, actions: MarkdownActions, pictures: Boolean = true) {
    val named = remember(block) { if (block is Block.Items || block is Block.Table) paths(block) else emptyList() }
    val grid = pictures && (named.size > 1 || block is Block.Table && named.isNotEmpty())
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        BlockBody(block, actions, pictures && !grid)
        if (grid) actions.pictures(named)
    }
}

@Composable
private fun BlockBody(block: Block, actions: MarkdownActions, pictures: Boolean) {
    when (block) {
        is Block.Para -> {
            RichText(block.text, Tm.Body, actions)
            if (pictures) {
                val named = remember(block) { ImagePaths.find(block.text.text) }
                if (named.isNotEmpty()) actions.pictures(named)
            }
        }
        is Block.Head -> {
            val (size, line) = HEADINGS.getOrElse(block.level - 1) { 17.sp to 26.sp }
            RichText(
                block.text, Tm.Body.copy(fontSize = size, lineHeight = line, fontWeight = FontWeight.SemiBold), actions,
                Modifier.padding(top = if (block.level <= 2) 6.dp else 2.dp),
            )
        }
        is Block.CodeBox -> CodeBox(block, actions)
        is Block.Items -> Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            block.items.forEachIndexed { i, item ->
                Row {
                    Text(
                        if (block.ordered) "${block.start + i}." else "•",
                        style = Tm.Body,
                        modifier = Modifier.widthIn(min = 26.dp).padding(end = 6.dp),
                    )
                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        item.forEach { BlockView(it, actions, pictures) }
                    }
                }
            }
        }
        is Block.Quote -> Row(Modifier.height(IntrinsicSize.Min)) {
            Box(Modifier.width(3.dp).fillMaxHeight().background(Tm.Line, RoundedCornerShape(2.dp)))
            Column(Modifier.padding(start = 14.dp).weight(1f), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                block.blocks.forEach { BlockView(it, actions, pictures) }
            }
        }
        is Block.Table -> TableView(block, actions)
        Block.Rule -> HorizontalDivider(Modifier.padding(vertical = 6.dp), color = Tm.Line)
    }
}

private val CHIP = SpanStyle(fontFamily = FontFamily.Monospace, fontSize = 0.92.em, color = Tm.Ink, background = Tm.Raised)
private val LINK = SpanStyle(color = Tm.Ink, textDecoration = TextDecoration.Underline)

/** Only what a browser or mail app can take: a "link" to anything else stays plain text. */
private fun opens(url: String) = url.startsWith("http://") || url.startsWith("https://") || url.startsWith("mailto:")

private fun Rich.annotated(openLink: (String) -> Unit): AnnotatedString = buildAnnotatedString {
    append(text)
    for (span in spans) {
        if (span.start >= span.end || span.end > text.length) continue
        when (span.style) {
            Style.BOLD -> addStyle(SpanStyle(fontWeight = FontWeight.Bold), span.start, span.end)
            Style.ITALIC -> addStyle(SpanStyle(fontStyle = FontStyle.Italic), span.start, span.end)
            Style.STRIKE -> addStyle(SpanStyle(textDecoration = TextDecoration.LineThrough), span.start, span.end)
            Style.CODE -> addStyle(CHIP, span.start, span.end)
            Style.LINK -> {
                val url = span.url
                if (url != null && opens(url)) {
                    addLink(LinkAnnotation.Url(url, TextLinkStyles(LINK)) { openLink(url) }, span.start, span.end)
                }
            }
        }
    }
}

@Composable
private fun RichText(rich: Rich, style: TextStyle, actions: MarkdownActions, modifier: Modifier = Modifier) {
    val text = remember(rich, actions) { rich.annotated(actions.openLink) }
    Text(text, modifier, style = style)
}

/** Fenced code: a sunk box with the language over it, a copy button and a full-screen button; long lines scroll sideways. */
@Composable
private fun CodeBox(block: Block.CodeBox, actions: MarkdownActions) {
    val label = Markdown.langLabel(block.lang)
    Surface(color = Tm.Sunken, shape = RoundedCornerShape(14.dp), modifier = Modifier.fillMaxWidth()) {
        Column {
            Row(Modifier.padding(start = 16.dp, end = 4.dp), verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
                Text(label, style = Tm.Mono.copy(color = Tm.Dim), modifier = Modifier.weight(1f))
                IconButton(onClick = { actions.copy(block.code) }) {
                    Icon(Glyphs.Copy, "Copy code", Modifier.size(20.dp), tint = Tm.Dim)
                }
                IconButton(onClick = { actions.showCode(label, block.code) }) {
                    Icon(Glyphs.Expand, "Full screen", Modifier.size(20.dp), tint = Tm.Dim)
                }
            }
            HorizontalDivider(color = Tm.Bg)
            Text(
                block.code,
                style = Tm.Mono,
                softWrap = false,
                modifier = Modifier.horizontalScroll(rememberScrollState()).padding(horizontal = 16.dp, vertical = 14.dp),
            )
        }
    }
}

/** The sizes the table was laid out with, for the lines drawn behind it. */
private class Grid {
    var widths = IntArray(0)
    var heights = IntArray(0)
}

/**
 * A GFM table: each column as wide as its widest cell (up to a limit, then the cell wraps), and
 * the whole table scrolling sideways inside itself when that is wider than the screen.
 */
@Composable
private fun TableView(table: Block.Table, actions: MarkdownActions) {
    val all = remember(table) { (listOf(table.head) + table.rows).filter { it.isNotEmpty() } }
    val cols = remember(all) { all.maxOfOrNull { it.size } ?: 0 }
    if (cols == 0) return
    val grid = remember(table) { Grid() }
    Box(Modifier.horizontalScroll(rememberScrollState())) {
        Layout(
            content = {
                all.forEachIndexed { r, cells ->
                    for (c in 0 until cols) {
                        Box(Modifier.padding(horizontal = 14.dp, vertical = 10.dp)) {
                            val cell = cells.getOrNull(c)
                            val head = r == 0 && table.head.isNotEmpty()
                            if (cell != null) RichText(cell, if (head) Tm.Body.copy(fontWeight = FontWeight.SemiBold) else Tm.Body, actions)
                        }
                    }
                }
            },
            modifier = Modifier.drawBehind {
                val w = size.width
                val h = size.height
                val stroke = 1.dp.toPx()
                var y = 0f
                drawLine(Tm.Line, Offset(0f, 0f), Offset(w, 0f), stroke)
                grid.heights.forEach {
                    y += it
                    drawLine(Tm.Line, Offset(0f, y), Offset(w, y), stroke)
                }
                var x = 0f
                drawLine(Tm.Line, Offset(0f, 0f), Offset(0f, h), stroke)
                grid.widths.forEach {
                    x += it
                    drawLine(Tm.Line, Offset(x, 0f), Offset(x, h), stroke)
                }
            },
        ) { measurables, _ ->
            val widest = 280.dp.roundToPx()
            val cells = measurables.map { it.measure(Constraints(maxWidth = widest)) }
            val rows = cells.size / cols
            val widths = IntArray(cols) { c -> (0 until rows).maxOf { r -> cells[r * cols + c].width } }
            val heights = IntArray(rows) { r -> (0 until cols).maxOf { c -> cells[r * cols + c].height } }
            grid.widths = widths
            grid.heights = heights
            layout(widths.sum(), heights.sum()) {
                var y = 0
                for (r in 0 until rows) {
                    var x = 0
                    for (c in 0 until cols) {
                        val cell = cells[r * cols + c]
                        val free = widths[c] - cell.width
                        val dx = when (table.align.getOrNull(c)) {
                            Align.CENTER -> free / 2
                            Align.END -> free
                            else -> 0
                        }
                        cell.place(x + dx, y)
                        x += widths[c]
                    }
                    y += heights[r]
                }
            }
        }
    }
}
