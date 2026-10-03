package dev.sagun.tmls.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import dev.sagun.tmls.update.Updater

/** The archbox app's update chip, as a bar above the site: tap to download and install. */
@Composable
fun UpdateBanner(updater: Updater) {
    val version = updater.offer ?: return
    val progress = updater.progress
    Surface(
        color = MaterialTheme.colorScheme.primaryContainer,
        modifier = Modifier
            .fillMaxWidth()
            .clickable(enabled = progress == null) { updater.install() }
            .semantics { contentDescription = "Update to $version available. Install." },
    ) {
        Column(Modifier.padding(horizontal = 16.dp, vertical = 10.dp)) {
            if (progress == null) {
                Text("Update available · Install", style = MaterialTheme.typography.labelLarge)
            } else {
                Text("Downloading update… $progress%", style = MaterialTheme.typography.labelLarge)
                LinearProgressIndicator(
                    progress = { progress / 100f },
                    modifier = Modifier.fillMaxWidth().padding(top = 6.dp),
                )
            }
        }
    }
}
