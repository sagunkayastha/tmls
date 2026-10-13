package dev.sagun.tmls.chat

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class MarkdownTest {
    private fun para(text: String): Rich = (Markdown.parse(text).single() as Block.Para).text

    @Test fun inlineStyles() {
        val rich = para("a **bold** and *it* `co de` ~~gone~~ end")
        assertEquals("a bold and it co de gone end", rich.text)
        assertEquals(
            listOf(Span(2, 6, Style.BOLD), Span(11, 13, Style.ITALIC), Span(14, 19, Style.CODE), Span(20, 24, Style.STRIKE)),
            rich.spans,
        )
    }

    @Test fun nestedStylesCoverTheirText() {
        val rich = para("**bold `code`**")
        assertEquals("bold code", rich.text)
        assertEquals(setOf(Span(5, 9, Style.CODE), Span(0, 9, Style.BOLD)), rich.spans.toSet())
    }

    @Test fun linksWrittenAndBare() {
        val rich = para("see [the docs](https://example.com/a) or https://example.org/x(1). Done")
        assertEquals("see the docs or https://example.org/x(1). Done", rich.text)
        assertEquals(
            listOf(Span(4, 12, Style.LINK, "https://example.com/a"), Span(16, 40, Style.LINK, "https://example.org/x(1)")),
            rich.spans,
        )
        assertEquals(listOf(Span(1, 20, Style.LINK, "https://example.com")), para("(https://example.com)").spans)
        // An address inside code is code, not a link.
        assertEquals(listOf(Span(0, 15, Style.CODE)), para("`http://x.test/a`").spans)
    }

    @Test fun lineBreaksStay() {
        assertEquals("one\ntwo", para("one\ntwo").text)
    }

    @Test fun headingsAndRules() {
        val blocks = Markdown.parse("# Top\n\n### Third\n\n---\n\ntext")
        assertEquals(Block.Head(1, Rich("Top")), blocks[0])
        assertEquals(Block.Head(3, Rich("Third")), blocks[1])
        assertEquals(Block.Rule, blocks[2])
        assertEquals(Block.Para(Rich("text")), blocks[3])
    }

    @Test fun fencedCodeKeepsItsTextAndLanguage() {
        val block = Markdown.parse("```sh title\nls -la\n  echo *hi*\n```").single() as Block.CodeBox
        assertEquals("sh", block.lang)
        assertEquals("ls -la\n  echo *hi*", block.code)
        assertEquals(Block.CodeBox("", "plain"), Markdown.parse("```\nplain\n```").single())
        assertEquals("Shell", Markdown.langLabel("sh"))
        assertEquals("Code", Markdown.langLabel(""))
        assertEquals("JSON", Markdown.langLabel("json"))
        assertEquals("Zig", Markdown.langLabel("zig"))
    }

    @Test fun listsBulletNumberedAndNested() {
        val bullets = Markdown.parse("- one\n- two\n  - inner\n- three").single() as Block.Items
        assertEquals(false, bullets.ordered)
        assertEquals(3, bullets.items.size)
        assertEquals(Block.Para(Rich("two")), bullets.items[1][0])
        val inner = bullets.items[1][1] as Block.Items
        assertEquals(listOf(listOf(Block.Para(Rich("inner")))), inner.items)

        val numbered = Markdown.parse("3. c\n4. d").single() as Block.Items
        assertTrue(numbered.ordered)
        assertEquals(3, numbered.start)
        assertEquals(2, numbered.items.size)
    }

    @Test fun quote() {
        val quote = Markdown.parse("> said\n> twice").single() as Block.Quote
        assertEquals(listOf(Block.Para(Rich("said\ntwice"))), quote.blocks)
    }

    @Test fun gfmTable() {
        val table = Markdown.parse("| Device | What |\n|:--|--:|\n| `pollen1` | Pi **home** |\n| b | c |").single() as Block.Table
        assertEquals(listOf(Rich("Device"), Rich("What")), table.head)
        assertEquals(listOf(Align.START, Align.END), table.align)
        assertEquals(2, table.rows.size)
        assertEquals(Rich("pollen1", listOf(Span(0, 7, Style.CODE))), table.rows[0][0])
        assertEquals(Rich("Pi home", listOf(Span(3, 7, Style.BOLD))), table.rows[0][1])
    }

    @Test fun htmlIsShownAsText() {
        assertEquals(Block.Para(Rich("<div>x</div>")), Markdown.parse("<div>x</div>").single())
        assertEquals("a <br> b", para("a <br> b").text)
    }

    @Test fun aLongReplyParses() {
        val text = (1..400).joinToString("\n\n") { "## Part $it\n\nSome `code` and **bold** text.\n\n- a\n- b\n\n```py\nprint($it)\n```" }
        assertEquals(400 * 4, Markdown.parse(text).size)
    }
}
