package dev.sagun.tmls.ui

import android.content.ActivityNotFoundException
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.os.Environment
import android.provider.MediaStore
import android.widget.Toast
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.spring
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.calculateCentroid
import androidx.compose.foundation.gestures.calculatePan
import androidx.compose.foundation.gestures.calculateZoom
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChanged
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.core.content.FileProvider
import coil.compose.AsyncImage
import coil.network.HttpException
import coil.request.CachePolicy
import coil.request.ImageRequest
import coil.size.Size
import dev.sagun.tmls.chat.ChatViewModel
import dev.sagun.tmls.chat.ImagePaths
import dev.sagun.tmls.chat.PictureBytes
import dev.sagun.tmls.chat.Uploads
import java.io.File
import java.io.IOException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * A picture of the chat: where its bytes come from ([url], cached as [cacheKey]) and what it is.
 * [path]: where it is on the session's host, when a message names it by path ([fromFile]: read
 * from there, so never kept on the phone's disk: the file may change). [keepKey]: it can be kept
 * (ChatViewModel.keepSent), with [keepBlob] and [keepName] for a picture of the record.
 */
internal data class Picture(
    val url: String,
    val cacheKey: String,
    val name: String,
    val path: String? = null,
    val fromFile: Boolean = false,
    val keepKey: String? = null,
    val keepBlob: String? = null,
    val keepName: String? = null,
)

/** Where it was kept for good, if it was: named by its kept path, or kept from this phone. */
internal fun Picture.kept(vm: ChatViewModel): String? =
    path?.takeIf(ImagePaths::isKept) ?: keepKey?.let { vm.keptSent[it] }?.takeIf { it.isNotEmpty() }

internal fun Picture.keep(vm: ChatViewModel) {
    val key = keepKey ?: return
    if (kept(vm) == null) vm.keepSent(key, blob = keepBlob, name = keepName)
}

/** Pictures a message names by path on [sessionKey]'s host. */
internal fun pathPictures(vm: ChatViewModel, sessionKey: String, paths: List<String>): List<Picture> = paths.mapNotNull { path ->
    val url = vm.fileUrl(sessionKey, path) ?: return@mapNotNull null
    val upload = Uploads.inText(path).isNotEmpty()
    Picture(url, "file:$sessionKey:$path", ImagePaths.name(path), path = path, fromFile = true, keepKey = path.takeIf { upload })
}

private fun request(context: Context, picture: Picture, full: Boolean = false): ImageRequest =
    ImageRequest.Builder(context).data(picture.url)
        .memoryCacheKey(picture.cacheKey + if (full) "/full" else "")
        .apply {
            if (full) size(Size.ORIGINAL)
            if (picture.fromFile) diskCachePolicy(CachePolicy.DISABLED) else diskCacheKey(picture.cacheKey)
        }
        .build()

private fun failure(error: Throwable): String = when ((error as? HttpException)?.response?.code) {
    404 -> "Not found"
    413 -> "Too large"
    401 -> "Log in again"
    else -> "Can't load"
}

/**
 * Pictures in a group: one at its own shape, several in a grid of squares. [user]: the user's own,
 * above their bubble on the right, bigger and rounder; else a reply's, on the left, each with its
 * file name under it.
 */
@Composable
internal fun PictureGroup(vm: ChatViewModel, pictures: List<Picture>, user: Boolean, onOpen: (Picture) -> Unit, modifier: Modifier = Modifier) {
    if (pictures.isEmpty()) return
    val density = LocalDensity.current.density
    val corner = if (user) 18.dp else 10.dp
    BoxWithConstraints(modifier.fillMaxWidth(), contentAlignment = if (user) Alignment.TopEnd else Alignment.TopStart) {
        val gap = 6.dp
        if (pictures.size == 1) {
            val picture = pictures[0]
            var natural by remember(picture) { mutableStateOf<Pair<Float, Float>?>(null) }
            val maxW = minOf(maxWidth.value, 260f)
            val maxH = if (user) 320f else 260f
            val size = natural?.let { (w, h) -> pictureSize(w, h, density, maxW, maxH) } ?: (if (user) 200f to 200f else 180f to 130f)
            Column(horizontalAlignment = if (user) Alignment.End else Alignment.Start) {
                Thumb(vm, picture, corner, crop = false, onOpen = onOpen, onSize = { w, h -> natural = w to h },
                    modifier = Modifier.size(size.first.dp, size.second.dp))
                if (!user) Caption(picture, size.first.dp)
            }
        } else {
            val cols = if (user || pictures.size == 2 || pictures.size == 4) 2 else 3
            val most = if (user) 140.dp else 120.dp
            val cell: Dp = minOf(most, (maxWidth - gap * (cols - 1)) / cols)
            Column(verticalArrangement = Arrangement.spacedBy(gap), horizontalAlignment = if (user) Alignment.End else Alignment.Start) {
                pictures.chunked(cols).forEach { line ->
                    Row(horizontalArrangement = Arrangement.spacedBy(gap)) {
                        line.forEach { picture ->
                            Column {
                                Thumb(vm, picture, corner, crop = true, onOpen = onOpen, modifier = Modifier.size(cell))
                                if (!user) Caption(picture, cell)
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun Caption(picture: Picture, width: Dp) {
    Text(
        picture.name, style = Tm.Small.copy(fontSize = 12.sp, lineHeight = 16.sp, color = Tm.Faint), maxLines = 1,
        overflow = TextOverflow.Ellipsis, modifier = Modifier.width(width).padding(top = 3.dp, start = 2.dp),
    )
}

/**
 * One picture: a tap opens it full screen; a long press says where it was kept, keeps one that
 * can be, or copies the path of one a reply named. A small bookmark on it once it is kept.
 */
@Composable
private fun Thumb(
    vm: ChatViewModel, picture: Picture, corner: Dp, crop: Boolean, onOpen: (Picture) -> Unit,
    modifier: Modifier = Modifier, onSize: ((Float, Float) -> Unit)? = null,
) {
    val context = LocalContext.current
    var failed by remember(picture) { mutableStateOf<String?>(null) }
    val req = remember(picture) { request(context, picture) }
    val kept = picture.kept(vm)
    Box(
        modifier.clip(RoundedCornerShape(corner)).background(Tm.Raised).pointerInput(picture) {
            detectTapGestures(
                onTap = { onOpen(picture) },
                onLongPress = { longPress(context, vm, picture) },
            )
        },
    ) {
        val reason = failed
        if (reason == null) {
            AsyncImage(
                model = req,
                imageLoader = vm.images,
                contentDescription = "Picture ${picture.name}. Tap for full screen.",
                contentScale = if (crop) ContentScale.Crop else ContentScale.Fit,
                onSuccess = { state ->
                    val size = state.painter.intrinsicSize
                    if (size.width > 0f && size.height > 0f) onSize?.invoke(size.width, size.height)
                },
                onError = { failed = failure(it.result.throwable) },
                modifier = Modifier.fillMaxSize(),
            )
        } else {
            Column(Modifier.fillMaxSize().padding(8.dp), verticalArrangement = Arrangement.Center, horizontalAlignment = Alignment.CenterHorizontally) {
                Icon(Glyphs.Photo, null, Modifier.size(20.dp), tint = Tm.Faint)
                Text(reason, style = Tm.Small.copy(fontSize = 12.sp, color = Tm.Dim), textAlign = TextAlign.Center, maxLines = 1)
            }
        }
        if (kept != null) {
            Box(
                Modifier.align(Alignment.TopEnd).padding(6.dp).size(22.dp).clip(CircleShape).background(Color.Black.copy(alpha = 0.55f)),
                contentAlignment = Alignment.Center,
            ) { Icon(Glyphs.KeepOn, "Kept", Modifier.size(13.dp), tint = Color.White) }
        }
    }
}

private fun longPress(context: Context, vm: ChatViewModel, picture: Picture) {
    val kept = picture.kept(vm)
    when {
        kept != null -> Toast.makeText(context, "Kept at ${Uploads.short(kept)}", Toast.LENGTH_LONG).show()
        picture.keepKey != null -> picture.keep(vm)
        picture.path != null -> copy(context, picture.path)
    }
}

/**
 * A picture over the whole screen, on black: pinch to zoom, drag to move, double tap to zoom in
 * (where tapped) and back, swipe down to close. Share, Save (Pictures/tmls) and, for one of the
 * user's own, Keep; at the bottom where it is on the host, or where it was kept.
 */
@Composable
internal fun PictureViewer(vm: ChatViewModel, picture: Picture, onClose: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val req = remember(picture) { request(context, picture, full = true) }
    var busy by remember { mutableStateOf(false) }
    val kept = picture.kept(vm)
    Dialog(onDismissRequest = onClose, properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        var scale by remember { mutableFloatStateOf(1f) }
        var offset by remember { mutableStateOf(Offset.Zero) }
        var pull by remember { mutableFloatStateOf(0f) }
        var pulling by remember { mutableStateOf(false) }
        val shownPull by animateFloatAsState(pull, if (pulling) snap() else spring(), label = "pull")
        var height by remember { mutableFloatStateOf(1f) }
        val fade = 1f - (shownPull / height).coerceIn(0f, 0.7f)
        Box(
            Modifier.fillMaxSize().background(Color.Black.copy(alpha = fade))
                .pointerInput(Unit) {
                    height = size.height.toFloat()
                    fun clamp(o: Offset, s: Float): Offset {
                        val reachX = size.width * (s - 1f) / 2f
                        val reachY = size.height * (s - 1f) / 2f
                        return Offset(o.x.coerceIn(-reachX, reachX), o.y.coerceIn(-reachY, reachY))
                    }
                    awaitEachGesture {
                        awaitFirstDown(requireUnconsumed = false)
                        var mode = 0  // 0 undecided, 1 zoom/move, 2 pull down
                        do {
                            val event = awaitPointerEvent()
                            val fingers = event.changes.count { it.pressed }
                            val zoom = event.calculateZoom()
                            val pan = event.calculatePan()
                            if (mode == 0 && (fingers > 1 || scale > 1.01f)) mode = 1
                            if (mode == 0 && fingers == 1 && pan.y > 6f && pan.y > kotlin.math.abs(pan.x)) mode = 2
                            if (mode == 1 && fingers > 0) {
                                val center = Offset(size.width / 2f, size.height / 2f)
                                val c = event.calculateCentroid()
                                val next = (scale * zoom).coerceIn(1f, 6f)
                                val moved = if (c == Offset.Unspecified) offset else c - center - (c - center - offset) * (next / scale)
                                offset = if (next <= 1f) Offset.Zero else clamp(moved + pan, next)
                                scale = next
                                event.changes.forEach { if (it.positionChanged()) it.consume() }
                            } else if (mode == 2) {
                                pulling = true
                                pull = (pull + pan.y).coerceAtLeast(0f)
                                event.changes.forEach { if (it.positionChanged()) it.consume() }
                            }
                        } while (event.changes.any { it.pressed })
                        if (mode == 2 && pull > size.height * 0.15f) onClose()
                        pulling = false
                        pull = 0f
                    }
                }
                .pointerInput(Unit) {
                    detectTapGestures(onDoubleTap = { tap ->
                        if (scale > 1f) {
                            scale = 1f
                            offset = Offset.Zero
                        } else {
                            val center = Offset(size.width / 2f, size.height / 2f)
                            val next = 2.5f
                            val o = (tap - center) * (1f - next)
                            val reachX = size.width * (next - 1f) / 2f
                            val reachY = size.height * (next - 1f) / 2f
                            offset = Offset(o.x.coerceIn(-reachX, reachX), o.y.coerceIn(-reachY, reachY))
                            scale = next
                        }
                    })
                },
        ) {
            AsyncImage(
                model = req,
                imageLoader = vm.images,
                contentDescription = "Picture ${picture.name}",
                contentScale = ContentScale.Fit,
                modifier = Modifier.fillMaxSize().graphicsLayer {
                    scaleX = scale
                    scaleY = scale
                    translationX = offset.x
                    translationY = offset.y + shownPull
                },
            )
            if (shownPull == 0f) {
                Row(
                    Modifier.fillMaxWidth().background(Color.Black.copy(alpha = 0.35f)).systemBarsPadding().padding(horizontal = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    IconButton(onClick = onClose) { Icon(Glyphs.Close, "Close", tint = Color.White) }
                    Spacer(Modifier.weight(1f))
                    if (busy) CircularProgressIndicator(Modifier.padding(end = 8.dp).size(18.dp), color = Color.White, strokeWidth = 2.dp)
                    if (picture.keepKey != null || kept != null) {
                        IconButton(onClick = { picture.keep(vm) }, enabled = kept == null) {
                            Icon(if (kept != null) Glyphs.KeepOn else Glyphs.Keep, if (kept != null) "Kept" else "Keep", tint = Color.White)
                        }
                    }
                    IconButton(onClick = {
                        if (!busy) scope.launch { busy = true; share(context, vm, picture); busy = false }
                    }) { Icon(Glyphs.Share, "Share", tint = Color.White) }
                    IconButton(onClick = {
                        if (!busy) scope.launch { busy = true; save(context, vm, picture); busy = false }
                    }) { Icon(Glyphs.Save, "Save to Pictures", tint = Color.White) }
                }
                val where = when {
                    kept != null -> "Kept: ${Uploads.short(kept)}"
                    picture.path != null -> Uploads.short(picture.path)
                    else -> null
                }
                if (where != null) {
                    Text(
                        where, style = Tm.Small.copy(color = Color.White.copy(alpha = 0.85f)), maxLines = 3, overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.align(Alignment.BottomStart).fillMaxWidth().background(Color.Black.copy(alpha = 0.45f))
                            .systemBarsPadding().padding(horizontal = 16.dp, vertical = 10.dp),
                    )
                }
            }
        }
    }
}

/** The picture's bytes, type and a file name for them; a toast and null when they can't be had. */
private suspend fun fetch(context: Context, vm: ChatViewModel, picture: Picture): Triple<ByteArray, String?, String>? = try {
    val (bytes, type) = vm.pictureBytes(picture.url)
    val mime = PictureBytes.sniff(bytes) ?: type?.substringBefore(';')
    Triple(bytes, mime, PictureBytes.fileName(picture.name, mime))
} catch (e: IOException) {
    Toast.makeText(context, "Couldn't get the picture: ${e.message}", Toast.LENGTH_LONG).show()
    null
}

/** Into the phone's Pictures/tmls through MediaStore (no permission needed from Android 10). */
private suspend fun save(context: Context, vm: ChatViewModel, picture: Picture) {
    val (bytes, mime, name) = fetch(context, vm, picture) ?: return
    val saved = withContext(Dispatchers.IO) {
        val resolver = context.contentResolver
        val values = ContentValues().apply {
            put(MediaStore.Images.Media.DISPLAY_NAME, name)
            put(MediaStore.Images.Media.MIME_TYPE, mime ?: "image/png")
            put(MediaStore.Images.Media.RELATIVE_PATH, Environment.DIRECTORY_PICTURES + "/tmls")
            put(MediaStore.Images.Media.IS_PENDING, 1)
        }
        val uri = try {
            resolver.insert(MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY), values)
        } catch (e: RuntimeException) {
            null
        } ?: return@withContext false
        try {
            resolver.openOutputStream(uri)?.use { it.write(bytes) } ?: throw IOException("no stream")
            resolver.update(uri, ContentValues().apply { put(MediaStore.Images.Media.IS_PENDING, 0) }, null, null)
            true
        } catch (e: Exception) {
            resolver.delete(uri, null, null)
            false
        }
    }
    Toast.makeText(context, if (saved) "Saved to Pictures/tmls/$name" else "Couldn't save the picture", Toast.LENGTH_LONG).show()
}

/** To another app, from a copy in the app's cache (FileProvider "shared/"). */
private suspend fun share(context: Context, vm: ChatViewModel, picture: Picture) {
    val (bytes, mime, name) = fetch(context, vm, picture) ?: return
    val file = withContext(Dispatchers.IO) {
        File(context.cacheDir, "shared").apply { mkdirs() }.resolve(name).apply { writeBytes(bytes) }
    }
    val uri = FileProvider.getUriForFile(context, "${context.packageName}.files", file)
    val send = Intent(Intent.ACTION_SEND).apply {
        type = mime ?: "image/*"
        putExtra(Intent.EXTRA_STREAM, uri)
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }
    try {
        context.startActivity(Intent.createChooser(send, null).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    } catch (e: ActivityNotFoundException) {
        Toast.makeText(context, "No app to share with", Toast.LENGTH_SHORT).show()
    }
}
