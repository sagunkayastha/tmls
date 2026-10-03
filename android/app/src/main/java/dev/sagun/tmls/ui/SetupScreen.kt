package dev.sagun.tmls.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import dev.sagun.tmls.ServerUrl
import kotlinx.coroutines.launch

/** First run, or "Change server": the home address and an optional away one. onConnect returns an error or null. */
@Composable
fun SetupScreen(home: String?, away: String?, message: String?, onConnect: suspend (home: String, away: String?) -> String?) {
    var homeText by remember { mutableStateOf(home ?: "") }
    var awayText by remember { mutableStateOf(away ?: "") }
    var error by remember { mutableStateOf(message) }
    var busy by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    fun connect() {
        val h = ServerUrl.normalize(homeText)
        val a = if (awayText.isBlank()) null else ServerUrl.normalize(awayText)
        error = when {
            h == null -> "Home address: like http://192.168.0.5:8794"
            awayText.isNotBlank() && a == null -> "Away address: like https://tmls.example.com"
            else -> null
        }
        if (h == null || error != null) return
        busy = true
        scope.launch {
            error = onConnect(h, a)
            busy = false
        }
    }

    Column(
        modifier = Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Text("Connect to tmls", style = MaterialTheme.typography.headlineSmall)
        OutlinedTextField(
            value = homeText, onValueChange = { homeText = it }, label = { Text("Home address") },
            placeholder = { Text("http://192.168.0.5:8794") },
            singleLine = true, modifier = Modifier.fillMaxWidth().semantics { contentDescription = "Home address" },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Next),
        )
        OutlinedTextField(
            value = awayText, onValueChange = { awayText = it }, label = { Text("Away address (optional)") },
            placeholder = { Text("https://tmls.example.com") },
            supportingText = { Text("Tried when home doesn't answer") },
            singleLine = true, modifier = Modifier.fillMaxWidth().semantics { contentDescription = "Away address" },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Go),
            keyboardActions = androidx.compose.foundation.text.KeyboardActions(onGo = { connect() }),
        )
        error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        Button(enabled = !busy, modifier = Modifier.fillMaxWidth(), onClick = ::connect) {
            if (busy) CircularProgressIndicator(modifier = Modifier.padding(2.dp)) else Text("Connect")
        }
    }
}
