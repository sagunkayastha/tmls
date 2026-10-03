package dev.sagun.tmls

import android.graphics.Color
import android.os.Bundle
import android.webkit.CookieManager
import android.webkit.WebView
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
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
        setContent {
            // Back with nothing left to close: to the background, like Home, so the session is still there.
            TmlsApp(servers, updater, onServer = { server = it }, onLeave = { moveTaskToBack(true) })
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
