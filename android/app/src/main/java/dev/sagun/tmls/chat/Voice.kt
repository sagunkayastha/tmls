package dev.sagun.tmls.chat

import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.speech.RecognitionListener
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer

/**
 * Speech to text with Android's own recognizer: on the phone itself when it can (Android 12+ with
 * an on-device model), else the default recognition service. One recognition at a time, on the
 * main thread. Nothing is sent anywhere by tmls: the words go into the message box only.
 */
class Voice(private val context: Context, private val sink: Sink) {
    interface Sink {
        /** The words so far ([final]: the recognizer is done with them). */
        fun onWords(text: String, final: Boolean)
        /** Listening ended; [error] says why when it was not a normal end. */
        fun onVoiceEnd(error: String?)
    }

    private var recognizer: SpeechRecognizer? = null
    private var onDevice = false
    private var heard = false
    /** Stop was asked: no falling back to another recognizer now. */
    private var stopping = false

    val listening: Boolean get() = recognizer != null

    /** Some recognizer exists on this phone. */
    fun available(): Boolean = SpeechRecognizer.isRecognitionAvailable(context) || onDeviceAvailable()

    private fun onDeviceAvailable(): Boolean =
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.S && SpeechRecognizer.isOnDeviceRecognitionAvailable(context)

    /** Start listening; false (and nothing started) when the phone has no recognizer. */
    fun start(): Boolean {
        cancel()
        heard = false
        stopping = false
        return when {
            onDeviceAvailable() -> begin(true)
            SpeechRecognizer.isRecognitionAvailable(context) -> begin(false)
            else -> false
        }
    }

    private fun begin(local: Boolean): Boolean {
        val r = try {
            if (local && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) SpeechRecognizer.createOnDeviceSpeechRecognizer(context)
            else SpeechRecognizer.createSpeechRecognizer(context)
        } catch (e: RuntimeException) {
            return if (local) begin(false) else false
        }
        onDevice = local
        recognizer = r
        r.setRecognitionListener(Listener(r))
        val intent = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH)
            .putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
            .putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, true)
            .putExtra(RecognizerIntent.EXTRA_CALLING_PACKAGE, context.packageName)
            .putExtra(RecognizerIntent.EXTRA_PREFER_OFFLINE, true)
            // a pause to think is not the end of the message
            .putExtra(RecognizerIntent.EXTRA_SPEECH_INPUT_COMPLETE_SILENCE_LENGTH_MILLIS, 2500L)
            .putExtra(RecognizerIntent.EXTRA_SPEECH_INPUT_POSSIBLY_COMPLETE_SILENCE_LENGTH_MILLIS, 2500L)
        try {
            r.startListening(intent)
        } catch (e: RuntimeException) {
            release()
            return if (local) begin(false) else false
        }
        return true
    }

    /** Stop listening and keep what was heard (the last words still arrive, final). */
    fun stop() {
        stopping = true
        try {
            recognizer?.stopListening()
        } catch (e: RuntimeException) {
            release()
            sink.onVoiceEnd(null)
        }
    }

    /** Stop at once; what was already put into the message box stays there. */
    fun cancel() {
        release()
    }

    private fun release() {
        val r = recognizer ?: return
        recognizer = null
        try {
            r.cancel()
            r.destroy()
        } catch (e: RuntimeException) {
            // already gone
        }
    }

    private inner class Listener(private val owner: SpeechRecognizer) : RecognitionListener {
        private fun current() = recognizer === owner

        private fun words(bundle: Bundle?): String? =
            bundle?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)?.firstOrNull()

        override fun onPartialResults(partialResults: Bundle?) {
            if (!current()) return
            val text = words(partialResults) ?: return
            if (text.isNotEmpty()) heard = true
            sink.onWords(text, false)
        }

        override fun onResults(results: Bundle?) {
            if (!current()) return
            val text = words(results).orEmpty()
            release()
            sink.onWords(text, true)
            sink.onVoiceEnd(if (text.isBlank() && !heard) Voice.message(SpeechRecognizer.ERROR_NO_MATCH) else null)
        }

        override fun onError(error: Int) {
            if (!current()) return
            release()
            // The on-device model is missing for this language, or its service failed before a word:
            // the default recognizer instead, without bothering the user.
            if (onDevice && !heard && !stopping && error in FALLBACK_ERRORS && begin(false)) return
            sink.onVoiceEnd(Voice.message(error))
        }

        override fun onReadyForSpeech(params: Bundle?) {}
        override fun onBeginningOfSpeech() {}
        override fun onRmsChanged(rmsdB: Float) {}
        override fun onBufferReceived(buffer: ByteArray?) {}
        override fun onEndOfSpeech() {}
        override fun onEvent(eventType: Int, params: Bundle?) {}
    }

    companion object {
        // ERROR_SERVER_DISCONNECTED 11, ERROR_LANGUAGE_NOT_SUPPORTED 12, ERROR_LANGUAGE_UNAVAILABLE 13,
        // ERROR_CANNOT_CHECK_SUPPORT 14 (constants from API 31+, written out for minSdk 29)
        private val FALLBACK_ERRORS = setOf(
            SpeechRecognizer.ERROR_CLIENT, SpeechRecognizer.ERROR_SERVER, SpeechRecognizer.ERROR_NETWORK,
            SpeechRecognizer.ERROR_NETWORK_TIMEOUT, 11, 12, 13, 14,
        )

        /** What went wrong, in plain words, for a SpeechRecognizer error code; null for none worth saying. */
        fun message(error: Int): String? = when (error) {
            SpeechRecognizer.ERROR_NO_MATCH, SpeechRecognizer.ERROR_SPEECH_TIMEOUT -> "Didn't catch that. Tap the mic and speak."
            SpeechRecognizer.ERROR_NETWORK, SpeechRecognizer.ERROR_NETWORK_TIMEOUT, SpeechRecognizer.ERROR_SERVER ->
                "Voice input needs the network on this phone (no offline speech model)."
            SpeechRecognizer.ERROR_AUDIO -> "The microphone could not be used. Is another app recording?"
            SpeechRecognizer.ERROR_INSUFFICIENT_PERMISSIONS -> "tmls may not use the microphone. Allow it in Android's settings for the app."
            SpeechRecognizer.ERROR_RECOGNIZER_BUSY -> "Speech recognition is busy. Try again in a moment."
            12, 13 -> "No speech model for this phone's language. Download one in Android's speech settings."
            SpeechRecognizer.ERROR_CLIENT -> null  // our own cancel
            else -> "Voice input failed (error $error)."
        }
    }
}
