package dev.sagun.tmls

import android.content.ContentUris
import android.content.Context
import android.graphics.Bitmap
import android.graphics.ImageDecoder
import android.provider.MediaStore
import android.util.Base64
import java.io.ByteArrayOutputStream
import org.json.JSONObject

/** The phone's newest screenshot, for sketchpad's "This phone's last screenshot". */
object Screenshots {
    /** Long side after scaling: plenty for a sketch, small enough to hand to the page quickly. */
    private const val MAX_SIDE = 2000

    /** JSON for the page: {name, mimeType, dataURL} or {error}. Needs the photos permission. */
    fun latestAsJson(context: Context): String = try {
        val resolver = context.contentResolver
        val projection = arrayOf(MediaStore.Images.Media._ID, MediaStore.Images.Media.DISPLAY_NAME)
        val where = "${MediaStore.Images.Media.RELATIVE_PATH} LIKE ?"
        val order = "${MediaStore.Images.Media.DATE_ADDED} DESC"
        resolver.query(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, projection, where, arrayOf("%Screenshots%"), order)
            ?.use { c ->
                if (!c.moveToFirst()) return error("No screenshots on this phone yet: press Power + Volume down, then try again.")
                val uri = ContentUris.withAppendedId(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, c.getLong(0))
                val bitmap = ImageDecoder.decodeBitmap(ImageDecoder.createSource(resolver, uri)) { decoder, info, _ ->
                    val (w, h) = scaled(info.size.width, info.size.height)
                    decoder.setTargetSize(w, h)
                }
                val bytes = ByteArrayOutputStream().also { bitmap.compress(Bitmap.CompressFormat.JPEG, 88, it) }.toByteArray()
                JSONObject()
                    .put("name", c.getString(1) ?: "screenshot.jpg")
                    .put("mimeType", "image/jpeg")
                    .put("dataURL", "data:image/jpeg;base64," + Base64.encodeToString(bytes, Base64.NO_WRAP))
                    .toString()
            } ?: error("Couldn't read the phone's pictures.")
    } catch (e: Exception) {
        error("Couldn't read the last screenshot: ${e.message ?: e.javaClass.simpleName}")
    }

    fun error(message: String): String = JSONObject().put("error", message).toString()

    /** Fits [w]x[h] inside MAX_SIDE, keeping the shape; never enlarges. */
    fun scaled(w: Int, h: Int): Pair<Int, Int> {
        val long = maxOf(w, h)
        if (long <= MAX_SIDE) return w to h
        val f = MAX_SIDE.toDouble() / long
        return maxOf(1, (w * f).toInt()) to maxOf(1, (h * f).toInt())
    }
}
