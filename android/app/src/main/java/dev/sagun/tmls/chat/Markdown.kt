package dev.sagun.tmls.chat

import org.commonmark.ext.gfm.strikethrough.Strikethrough
import org.commonmark.ext.gfm.strikethrough.StrikethroughExtension
import org.commonmark.ext.gfm.tables.TableBody
import org.commonmark.ext.gfm.tables.TableBlock
import org.commonmark.ext.gfm.tables.TableCell
import org.commonmark.ext.gfm.tables.TableHead
import org.commonmark.ext.gfm.tables.TableRow
import org.commonmark.ext.gfm.tables.TablesExtension
import org.commonmark.node.BlockQuote
import org.commonmark.node.BulletList
import org.commonmark.node.Code
import org.commonmark.node.Emphasis
import org.commonmark.node.FencedCodeBlock
import org.commonmark.node.HardLineBreak
import org.commonmark.node.Heading
import org.commonmark.node.HtmlBlock
import org.commonmark.node.HtmlInline
import org.commonmark.node.Image
import org.commonmark.node.IndentedCodeBlock
import org.commonmark.node.Link
import org.commonmark.node.ListItem
import org.commonmark.node.Node
import org.commonmark.node.OrderedList
import org.commonmark.node.Paragraph
import org.commonmark.node.SoftLineBreak
import org.commonmark.node.StrongEmphasis
import org.commonmark.node.Text
import org.commonmark.node.ThematicBreak
import org.commonmark.parser.Parser

enum class Style { BOLD, ITALIC, STRIKE, CODE, LINK }

/** [style] over text[start, end); [url] for a LINK. */
data class Span(val start: Int, val end: Int, val style: Style, val url: String? = null)

/** A run of text with its styles: what one paragraph, heading or table cell holds. */
data class Rich(val text: String, val spans: List<Span> = emptyList())

enum class Align { START, CENTER, END }

/** A reply as the screen draws it: a small model of Markdown's blocks, with no Android in it. */
sealed interface Block {
    data class Para(val text: Rich) : Block
    data class Head(val level: Int, val text: Rich) : Block
    data class CodeBox(val lang: String, val code: String) : Block
    /** [items]: each item's own blocks (a nested list is a block of its item). */
    data class Items(val ordered: Boolean, val start: Int, val items: List<List<Block>>) : Block
    data class Quote(val blocks: List<Block>) : Block
    data class Table(val head: List<Rich>, val rows: List<List<Rich>>, val align: List<Align>) : Block
    data object Rule : Block
}

/** Markdown (CommonMark + GFM tables and strikethrough) to [Block]s. */
object Markdown {
    // A Parser is immutable once built: one for every thread.
    private val parser: Parser = Parser.builder()
        .extensions(listOf(TablesExtension.create(), StrikethroughExtension.create()))
        .build()

    fun parse(text: String): List<Block> = blocks(parser.parse(text))

    private fun children(node: Node): Sequence<Node> = generateSequence(node.firstChild) { it.next }

    private fun blocks(parent: Node): List<Block> = children(parent).mapNotNull(::block).toList()

    private fun block(node: Node): Block? = when (node) {
        is Paragraph -> rich(node).takeIf { it.text.isNotEmpty() }?.let { Block.Para(it) }
        is Heading -> Block.Head(node.level, rich(node))
        is FencedCodeBlock -> Block.CodeBox(node.info.orEmpty().trim().substringBefore(' '), node.literal.orEmpty().trimEnd('\n'))
        is IndentedCodeBlock -> Block.CodeBox("", node.literal.orEmpty().trimEnd('\n'))
        is BulletList -> Block.Items(false, 1, children(node).filterIsInstance<ListItem>().map(::blocks).toList())
        is OrderedList -> Block.Items(true, node.markerStartNumber ?: 1, children(node).filterIsInstance<ListItem>().map(::blocks).toList())
        is BlockQuote -> Block.Quote(blocks(node))
        is ThematicBreak -> Block.Rule
        is TableBlock -> table(node)
        // Raw HTML is shown as the text it is: nothing here renders HTML.
        is HtmlBlock -> Block.Para(Rich(node.literal.orEmpty().trimEnd('\n')))
        else -> rich(node).takeIf { it.text.isNotEmpty() }?.let { Block.Para(it) }
    }

    private fun table(node: TableBlock): Block.Table {
        var head = emptyList<Rich>()
        var align = emptyList<Align>()
        val rows = ArrayList<List<Rich>>()
        for (part in children(node)) {
            for (row in children(part).filterIsInstance<TableRow>()) {
                val cells = children(row).filterIsInstance<TableCell>().toList()
                if (part is TableHead) {
                    head = cells.map(::rich)
                    align = cells.map {
                        when (it.alignment) {
                            TableCell.Alignment.CENTER -> Align.CENTER
                            TableCell.Alignment.RIGHT -> Align.END
                            else -> Align.START
                        }
                    }
                } else if (part is TableBody) {
                    rows.add(cells.map(::rich))
                }
            }
        }
        return Block.Table(head, rows, align)
    }

    /** The inline content under [node] as one styled string. */
    private fun rich(node: Node): Rich {
        val text = StringBuilder()
        val spans = ArrayList<Span>()
        inline(node, text, spans, inLink = false)
        return Rich(text.toString(), spans)
    }

    private fun inline(parent: Node, text: StringBuilder, spans: MutableList<Span>, inLink: Boolean) {
        for (node in children(parent)) {
            val start = text.length
            when (node) {
                is Text -> if (inLink) text.append(node.literal) else linkify(node.literal.orEmpty(), text, spans)
                is Code -> {
                    text.append(node.literal)
                    spans.add(Span(start, text.length, Style.CODE))
                }
                is Emphasis -> styled(node, text, spans, inLink, Style.ITALIC)
                is StrongEmphasis -> styled(node, text, spans, inLink, Style.BOLD)
                is Strikethrough -> styled(node, text, spans, inLink, Style.STRIKE)
                is Link -> {
                    inline(node, text, spans, inLink = true)
                    if (text.length == start) text.append(node.destination)
                    spans.add(Span(start, text.length, Style.LINK, node.destination))
                }
                // No pictures from the open web in a chat: an image is a link to it.
                is Image -> {
                    inline(node, text, spans, inLink = true)
                    if (text.length == start) text.append(node.destination)
                    spans.add(Span(start, text.length, Style.LINK, node.destination))
                }
                // Claude writes one line per thought: a line break stays a line break.
                is SoftLineBreak, is HardLineBreak -> text.append('\n')
                is HtmlInline -> text.append(node.literal)
                else -> inline(node, text, spans, inLink)
            }
        }
    }

    private fun styled(node: Node, text: StringBuilder, spans: MutableList<Span>, inLink: Boolean, style: Style) {
        val start = text.length
        inline(node, text, spans, inLink)
        if (text.length > start) spans.add(Span(start, text.length, style))
    }

    private val URL = Regex("""https?://[^\s<>"'`]+""")

    /** A bare address in running text is a link too; punctuation that ends the sentence is not part of it. */
    private fun linkify(literal: String, text: StringBuilder, spans: MutableList<Span>) {
        var at = 0
        for (m in URL.findAll(literal)) {
            var url = m.value.trimEnd('.', ',', ';', ':', '!', '?')
            while (url.endsWith(')') && url.count { it == ')' } > url.count { it == '(' }) url = url.dropLast(1)
            if (url.length <= "https://".length) continue
            text.append(literal, at, m.range.first)
            val start = text.length
            text.append(url)
            spans.add(Span(start, text.length, Style.LINK, url))
            at = m.range.first + url.length
        }
        text.append(literal, at, literal.length)
    }

    /** The label over a code box: the fence's language, spelled out for the common ones. */
    fun langLabel(lang: String): String = when (lang.lowercase()) {
        "" -> "Code"
        "sh", "bash", "zsh", "shell", "console" -> "Shell"
        "py", "python" -> "Python"
        "js", "javascript" -> "JavaScript"
        "ts", "typescript" -> "TypeScript"
        "kt", "kotlin" -> "Kotlin"
        "rs", "rust" -> "Rust"
        "md", "markdown" -> "Markdown"
        "json", "yaml", "toml", "sql", "html", "css", "xml" -> lang.uppercase()
        "yml" -> "YAML"
        "diff", "patch" -> "Diff"
        "text", "txt", "plaintext" -> "Text"
        else -> lang.replaceFirstChar { it.uppercase() }
    }
}
