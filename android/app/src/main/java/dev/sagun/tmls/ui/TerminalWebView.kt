package dev.sagun.tmls.ui

import android.content.Context
import android.text.InputType
import android.util.Log
import android.view.KeyEvent
import android.view.View
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager
import android.webkit.WebView
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import org.json.JSONObject

/**
 * The site's WebView, plus a native input for the terminal.
 *
 * Typing into xterm.js's hidden textarea with an Android keyboard doubles text: the keyboard
 * "composes" words for suggestions, xterm resets the textarea as the screen updates, and the
 * keyboard re-sends what it was composing. So while the terminal has focus (the page says
 * "ime:terminal"), the keyboard talks to [keys] instead, a plain native input that turns what it
 * gets into exact keystrokes for the page (tmlsType in app.js) — the way Termux does it. Other
 * fields (login, Sketch's message) keep the WebView's own input with suggestions.
 */
class TerminalWebView(context: Context) : WebView(context) {
    val keys = TerminalKeys(context) { text ->
        evaluateJavascript("typeof tmlsType === 'function' && tmlsType(${JSONObject.quote(text)})", null)
    }

    /** Nothing to type into (the list, Copy's view): the keyboard goes away, focus stays put. */
    fun keyboardAway() {
        context.getSystemService(InputMethodManager::class.java)?.hideSoftInputFromWindow(windowToken, 0)
    }

    /** The key bar's ⌨: the keyboard up for the terminal, or away. */
    fun toggleKeyboard() {
        val shown = ViewCompat.getRootWindowInsets(rootView)?.isVisible(WindowInsetsCompat.Type.ime()) == true
        Log.i("tmls", "keyboard toggle: shown=$shown")
        if (shown) keyboardAway() else terminalInput(true)
    }

    /** The terminal focused: keyboard to [keys]; anything else: back to the page. */
    fun terminalInput(on: Boolean) {
        val imm = context.getSystemService(InputMethodManager::class.java) ?: return
        if (on) {
            // A touch on the page (scrolling, the key bar) mustn't take focus back from [keys]:
            // the page would see its terminal focused again and bring the keyboard back.
            isFocusableInTouchMode = false
            keys.requestFocus()
            imm.showSoftInput(keys, 0)
        } else {
            isFocusableInTouchMode = true
            if (keys.hasFocus()) {
                requestFocus()
                imm.showSoftInput(this, 0)  // the field the page focused
            }
        }
    }
}

/** An invisible view that takes keyboard input as plain characters and hands them on. */
class TerminalKeys(context: Context, private val send: (String) -> Unit) : View(context) {
    init {
        isFocusable = true
        isFocusableInTouchMode = true
    }

    override fun onCheckIsTextEditor() = true

    override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection {
        outAttrs.inputType = InputType.TYPE_CLASS_TEXT or
            InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD or
            InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
        outAttrs.imeOptions = EditorInfo.IME_FLAG_NO_EXTRACT_UI or EditorInfo.IME_FLAG_NO_FULLSCREEN or
            EditorInfo.IME_ACTION_NONE or EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING
        return Connection()
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        val s = keyText(event) ?: return super.onKeyDown(keyCode, event)
        send(s)
        return true
    }

    /**
     * Some keyboards still "compose" a word and re-send it as it grows. Tracking what is being
     * composed and sending only the difference (backspaces for what changed, then the new tail)
     * means each character reaches the terminal exactly once.
     */
    private inner class Connection : BaseInputConnection(this@TerminalKeys, false) {
        private var composing = ""

        private fun replace(old: String, new: String) {
            val out = change(old, new)
            if (out.isNotEmpty()) send(out)
        }

        override fun commitText(text: CharSequence, newCursorPosition: Int): Boolean {
            replace(composing, text.toString())
            composing = ""
            return true
        }

        override fun setComposingText(text: CharSequence, newCursorPosition: Int): Boolean {
            replace(composing, text.toString())
            composing = text.toString()
            return true
        }

        override fun finishComposingText(): Boolean {
            composing = ""
            return true
        }

        override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
            if (beforeLength > 0) send("\u007f".repeat(beforeLength))
            if (afterLength > 0) send("\u001b[3~".repeat(afterLength))
            return true
        }

        override fun sendKeyEvent(event: KeyEvent): Boolean {
            if (event.action == KeyEvent.ACTION_DOWN) keyText(event)?.let(send)
            return true
        }

        override fun performEditorAction(actionCode: Int): Boolean {
            send("\r")
            return true
        }
    }

    companion object {
        /** Keystrokes that turn [old] into [new] on a terminal: backspaces, then the new tail. */
        fun change(old: String, new: String): String {
            val keep = old.commonPrefixWith(new).length
            return ("\u007f".repeat(old.length - keep) + new.substring(keep)).replace('\n', '\r')
        }

        /** What a key press means to a terminal, or null for keys it doesn't use. */
        fun keyText(event: KeyEvent): String? = when (event.keyCode) {
            KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER -> "\r"
            KeyEvent.KEYCODE_DEL -> "\u007f"
            KeyEvent.KEYCODE_FORWARD_DEL -> "\u001b[3~"
            KeyEvent.KEYCODE_TAB -> "\t"
            KeyEvent.KEYCODE_ESCAPE -> "\u001b"
            KeyEvent.KEYCODE_DPAD_UP -> "\u001b[A"
            KeyEvent.KEYCODE_DPAD_DOWN -> "\u001b[B"
            KeyEvent.KEYCODE_DPAD_RIGHT -> "\u001b[C"
            KeyEvent.KEYCODE_DPAD_LEFT -> "\u001b[D"
            KeyEvent.KEYCODE_MOVE_HOME -> "\u001b[H"
            KeyEvent.KEYCODE_MOVE_END -> "\u001b[F"
            KeyEvent.KEYCODE_PAGE_UP -> "\u001b[5~"
            KeyEvent.KEYCODE_PAGE_DOWN -> "\u001b[6~"
            else -> {
                val c = event.getUnicodeChar(event.metaState and KeyEvent.META_CTRL_MASK.inv())
                when {
                    c == 0 -> null
                    event.isCtrlPressed && c.toChar().isLetter() -> (c.toChar().uppercaseChar().code and 0x1f).toChar().toString()
                    else -> String(Character.toChars(c))
                }
            }
        }
    }
}
