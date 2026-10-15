package dev.sagun.tmls

import android.content.Intent
import android.graphics.Color
import android.os.Bundle
import android.webkit.CookieManager
import android.webkit.WebView
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import dev.sagun.tmls.ui.ShareInbox
import dev.sagun.tmls.ui.TmlsApp
import dev.sagun.tmls.update.Updater

class MainActivity : ComponentActivity() {
    private lateinit var updater: Updater
    /** The tmls-web that answered; the updater asks the same one. */
    private var server: String? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Drawn behind the bars, light icons on the terminal's dark.
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
        )
        WebView.setWebContentsDebuggingEnabled(BuildConfig.DEBUG || BuildConfig.BUILD_TYPE == "profile")
        updater = Updater(this) { server }
        val servers = Servers(this)
        // Opened from another app's share sheet (not again after a recreate: it was taken then).
        if (savedInstanceState == null) takeShare(intent)
        setContent {
            // Back with nothing left to close: to the background, like Home, so the session is still there.
            TmlsApp(servers, updater, onServer = { server = it }, onLeave = { moveTaskToBack(true) })
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        takeShare(intent)
    }

    private fun takeShare(intent: Intent?) {
        // Reopened from Recents: the old share intent again, its file grants long gone.
        if (intent == null || intent.flags and Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY != 0) return
        ShareInbox.read(this, intent)?.let {
            ShareInbox.pending = it
            setIntent(Intent(intent).setAction(Intent.ACTION_MAIN))  // taken: a recreate must not take it again
        }
    }

    override fun onStart() {
        super.onStart()
        updater.checkIfDue()
    }

    override fun onStop() {
        CookieManager.getInstance().flush()
        super.onStop()
    }

    override fun onDestroy() {
        updater.close()
        super.onDestroy()
    }
}
