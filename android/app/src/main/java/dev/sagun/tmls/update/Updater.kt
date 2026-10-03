package dev.sagun.tmls.update

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageInstaller
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.util.Log
import android.webkit.CookieManager
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.content.ContextCompat
import androidx.core.content.IntentCompat
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.io.IOException
import java.security.DigestOutputStream
import java.security.MessageDigest
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import okhttp3.OkHttpClient
import okhttp3.Request

/**
 * In-app update, the archbox app's design (hub/android Updater.java) ported to tmls (by way of fin). Asks the
 * server which build is published (/app/latest.json), offers a banner when it is newer, and on a
 * tap downloads /app/tmls.apk with the WebView's session cookie, verifies its length and SHA-256
 * against the manifest, and hands it to Android's PackageInstaller. Android still asks the user to
 * confirm; nothing here installs silently, and nothing downloads without the tap.
 */
/** [baseUrl] is the tmls-web that answered (home or away), or null before one has. */
class Updater(private val activity: ComponentActivity, private val baseUrl: () -> String?) {
    /** Version name of the build on offer, or null. The banner shows while this is set. */
    var offer by mutableStateOf<String?>(null)
        private set

    /** 0-99 while an update downloads, else null. */
    var progress by mutableStateOf<Int?>(null)
        private set

    private val app: Context = activity.applicationContext
    private val prefs = app.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
    private val main = Handler(Looper.getMainLooper())
    private val io = Executors.newSingleThreadExecutor()
    private val checkClient = client(CHECK_TIMEOUT_S, CHECK_TIMEOUT_S)
    private val apkClient = client(APK_CONNECT_S, APK_READ_S)
    private val results = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) = onInstallResult(intent)
    }
    private var registered = false
    private var closed = false
    /** The manifest the banner is currently offering; null when no offer is outstanding. */
    private var offered: UpdateRules.Manifest? = null
    private var working = false

    /** The site has loaded: quiet unless there is something to offer. */
    fun checkNow() = check(manual = false)

    /** The site's "Check for updates" (/app/update): always answers, and installs without the banner. */
    fun checkManual() = check(manual = true)

    /** Returning to the foreground: ask only if the last check is old enough. */
    fun checkIfDue() {
        if (System.currentTimeMillis() - prefs.getLong(KEY_CHECKED, 0) >= CHECK_INTERVAL_MS) check(manual = false)
    }

    /** The banner's Install tap. */
    fun install() {
        if (closed || working) return
        val m = offered ?: return
        if (!app.packageManager.canRequestPackageInstalls()) {
            // One-time trip to Android's own "Install unknown apps" toggle; the next tap continues.
            activity.startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${app.packageName}")))
            return
        }
        val base = baseUrl() ?: return
        working = true
        progress = 0
        val url = "$base/app/tmls.apk"
        io.execute {
            val failure = download(m, url)
            main.post {
                working = false
                progress = null
                if (closed) return@post
                if (failure != null) {
                    Log.w(TAG, "update failed: $failure")
                    toast("Update failed: $failure")
                    return@post
                }
                commit(m)
            }
        }
    }

    fun close() {
        closed = true
        unregister()
        main.removeCallbacksAndMessages(null)
        io.shutdownNow()
    }

    // ---- checking ---------------------------------------------------------------------------

    private fun check(manual: Boolean) {
        if (closed) return
        if (working) {
            if (manual) toast("An update is already running")
            return
        }
        // No server yet: not a check, so the 6 h clock does not start.
        val base = baseUrl() ?: return
        val url = "$base/app/latest.json"
        io.execute {
            val body = fetchText(url)
            main.post { onManifest(body, manual) }
        }
    }

    private fun onManifest(body: String?, manual: Boolean) {
        if (closed) return
        prefs.edit().putLong(KEY_CHECKED, System.currentTimeMillis()).apply()
        val m = UpdateRules.parse(body)
        if (m == null) {
            // Signed out, nothing published, or a body we do not understand. Logged either way:
            // a check that fails silently cannot be told from one that found nothing.
            Log.w(TAG, "update check: no usable manifest")
            if (manual) toast("Could not check for updates")
            return
        }
        val installed = installedCode()
        if (!UpdateRules.isNewer(installed, m.versionCode)) {
            Log.i(TAG, "update check: up to date at $installed")
            offered = null
            offer = null
            if (manual) toast("Already up to date")
            return
        }
        Log.i(TAG, "update check: $installed -> ${m.versionCode}")
        offered = m
        offer = m.versionName
        if (manual) install()
    }

    private fun installedCode(): Long = try {
        app.packageManager.getPackageInfo(app.packageName, 0).longVersionCode
    } catch (e: Exception) {
        Long.MAX_VALUE // unknown: never offer an update we cannot compare against
    }

    // ---- installing -------------------------------------------------------------------------

    /** Null on success, else the reason. A failed attempt never leaves its partial file behind. */
    private fun download(m: UpdateRules.Manifest, url: String): String? {
        val failure = fetch(m, url)
        if (failure != null) {
            val partial = apkFile()
            if (partial.exists() && !partial.delete()) Log.w(TAG, "could not clear the update cache")
        }
        return failure
    }

    private fun fetch(m: UpdateRules.Manifest, url: String): String? {
        val file = apkFile()
        return try {
            val dir = file.parentFile
            if (dir != null && !dir.isDirectory && !dir.mkdirs()) return "no room for the download"
            apkClient.newCall(request(url)).execute().use { r ->
                if (r.code != 200) return "HTTP ${r.code}"
                val body = r.body ?: return "the download did not finish"
                val sha = MessageDigest.getInstance("SHA-256")
                body.byteStream().use { input ->
                    DigestOutputStream(FileOutputStream(file), sha).use { out ->
                        UpdateRules.copy(input, out, m.size) { pct -> main.post { progress = pct } }
                    }
                }
                if (!UpdateRules.digestMatches(m.sha256, UpdateRules.hex(sha.digest()))) {
                    return "the file did not match its checksum"
                }
                null
            }
        } catch (e: IOException) {
            "the download did not finish"
        } catch (e: RuntimeException) {
            "the download did not finish"
        }
    }

    private fun commit(m: UpdateRules.Manifest) {
        val installer = app.packageManager.packageInstaller
        var session: PackageInstaller.Session? = null
        try {
            register()
            val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL)
            params.setAppPackageName(app.packageName)
            val id = installer.createSession(params)
            val open = installer.openSession(id)
            session = open
            open.openWrite("tmls", 0, m.size).use { out ->
                FileInputStream(apkFile()).use { input -> UpdateRules.copy(input, out, m.size) {} }
                open.fsync(out)
            }
            val pending = PendingIntent.getBroadcast(
                app, id, Intent(ACTION_RESULT).setPackage(app.packageName),
                PendingIntent.FLAG_MUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
            open.commit(pending.intentSender)
        } catch (e: IOException) {
            handOffFailed(session)
        } catch (e: RuntimeException) {
            handOffFailed(session)
        } finally {
            session?.close()
        }
    }

    private fun handOffFailed(session: PackageInstaller.Session?) {
        session?.abandon()
        forget()
        Log.w(TAG, "update could not be handed to the installer")
        toast("Update failed: Android would not take the file")
    }

    private fun onInstallResult(intent: Intent) {
        val status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)
        if (status == PackageInstaller.STATUS_PENDING_USER_ACTION) {
            // Android's own "Update tmls?" dialog. This is the tap the user makes.
            IntentCompat.getParcelableExtra(intent, Intent.EXTRA_INTENT, Intent::class.java)?.let {
                it.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                activity.startActivity(it)
            }
            return
        }
        forget()
        if (status == PackageInstaller.STATUS_SUCCESS) return // Android replaces the running app
        val message = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE)
        Log.w(TAG, "install failed, status $status")
        toast("Update failed: ${message ?: "Android refused it"}")
    }

    // ---- plumbing ---------------------------------------------------------------------------

    private fun apkFile() = File(File(app.cacheDir, "update"), "tmls.apk")

    /** The cache copy has done its job; the digest is re-checked before any future install. */
    private fun forget() {
        unregister()
        val file = apkFile()
        if (file.exists() && !file.delete()) Log.w(TAG, "could not clear the update cache")
        offered = null
        offer = null
    }

    private fun register() {
        if (registered) return
        ContextCompat.registerReceiver(activity, results, IntentFilter(ACTION_RESULT), ContextCompat.RECEIVER_NOT_EXPORTED)
        registered = true
    }

    private fun unregister() {
        if (!registered) return
        registered = false
        try {
            activity.unregisterReceiver(results)
        } catch (ignored: IllegalArgumentException) {
            // already gone
        }
    }

    private fun fetchText(url: String): String? = try {
        checkClient.newCall(request(url)).execute().use { r ->
            if (r.code != 200) null else r.peekBody(UpdateRules.MAX_BODY + 1L).string()
        }
    } catch (e: IOException) {
        null
    } catch (e: RuntimeException) {
        null
    }

    /** The WebView's own session cookie for this URL, exactly what the site sees. */
    private fun request(url: String): Request {
        val builder = Request.Builder().url(url).header("Cache-Control", "no-cache")
        val cookie = CookieManager.getInstance().getCookie(url)
        if (!cookie.isNullOrEmpty()) builder.header("Cookie", cookie)
        return builder.build()
    }

    private fun toast(text: String) {
        main.post { Toast.makeText(app, text, Toast.LENGTH_SHORT).show() }
    }

    private companion object {
        const val TAG = "tmls"
        const val PREFS = "updater"
        const val KEY_CHECKED = "checked_at"
        const val ACTION_RESULT = "dev.sagun.tmls.INSTALL_RESULT"
        const val CHECK_INTERVAL_MS = 6L * 60 * 60 * 1000
        const val CHECK_TIMEOUT_S = 5L
        const val APK_CONNECT_S = 15L
        const val APK_READ_S = 30L

        // Redirects off: a signed-out request is a 303 to /login, which must read as "no manifest".
        fun client(connectS: Long, readS: Long): OkHttpClient = OkHttpClient.Builder()
            .connectTimeout(connectS, TimeUnit.SECONDS)
            .readTimeout(readS, TimeUnit.SECONDS)
            .followRedirects(false)
            .build()
    }
}
