package dev.sagun.tmls.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalDrawerSheet
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.sagun.tmls.BuildConfig
import dev.sagun.tmls.chat.ChatViewModel
import dev.sagun.tmls.chat.HostInfo
import dev.sagun.tmls.chat.Row as SessionRow
import dev.sagun.tmls.chat.Sessions

/**
 * Every session of every host, by host, the ones waiting for the user first; a tap picks one.
 * At its foot: New session and the app's version.
 */
@Composable
fun SessionDrawer(vm: ChatViewModel, onPick: (SessionRow) -> Unit, onNew: (HostInfo) -> Unit) {
    ModalDrawerSheet(drawerContainerColor = Tm.Bg, drawerContentColor = Tm.Text, modifier = Modifier.widthIn(max = 340.dp)) {
        Text("Sessions", style = Tm.Body.copy(fontSize = 22.sp, fontWeight = FontWeight.SemiBold), modifier = Modifier.padding(start = 24.dp, top = 20.dp, bottom = 8.dp))
        val groups = vm.groups
        LazyColumn(Modifier.weight(1f).fillMaxWidth()) {
            if (groups.isEmpty()) {
                item("empty") {
                    Text(
                        if (vm.listed) "No sessions." else "Loading…",
                        style = Tm.Small, modifier = Modifier.padding(horizontal = 24.dp, vertical = 16.dp),
                    )
                }
            }
            groups.forEach { group ->
                item("h:" + group.host, contentType = "host") {
                    Text(
                        if (group.online) group.label else "${group.label} · offline",
                        style = Tm.Small.copy(color = Tm.Faint, fontSize = 13.sp),
                        modifier = Modifier.padding(start = 24.dp, end = 24.dp, top = 18.dp, bottom = 6.dp),
                    )
                }
                items(group.rows, key = { it.key }, contentType = { "row" }) { row ->
                    SessionLine(row, selected = row.key == vm.selected) { onPick(row) }
                }
            }
        }
        HorizontalDivider(color = Tm.Line)
        NewSession(Sessions.creatable(vm.hosts), onNew)
        Text(
            "tmls ${BuildConfig.VERSION_NAME}", style = Tm.Small.copy(color = Tm.Faint, fontSize = 12.sp),
            modifier = Modifier.padding(start = 24.dp, bottom = 12.dp),
        )
    }
}

/** A session: its status mark, its name, its second line, and how full Claude's context is. */
@Composable
private fun SessionLine(row: SessionRow, selected: Boolean, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp).clip(RoundedCornerShape(14.dp))
            .background(if (selected) Tm.Raised else Tm.Bg)
            .clickable(role = Role.Button, onClick = onClick)
            .padding(start = 12.dp, end = 14.dp, top = 10.dp, bottom = 10.dp)
            .alpha(if (row.online) 1f else 0.5f),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            Sessions.glyph(row.mark), color = Tm.mark(row.mark), fontSize = 15.sp, textAlign = TextAlign.Center,
            fontWeight = if (row.waiting) FontWeight.Bold else FontWeight.Normal, modifier = Modifier.width(24.dp),
        )
        Column(Modifier.weight(1f).padding(start = 6.dp)) {
            Text(row.name, style = Tm.Body.copy(fontSize = 16.sp, lineHeight = 22.sp), maxLines = 1, overflow = TextOverflow.Ellipsis)
            val line = if (row.waiting && row.line.isBlank()) "waiting for you" else row.line
            if (line.isNotBlank()) {
                Text(line, style = Tm.Small.copy(fontSize = 13.sp, lineHeight = 18.sp, color = if (row.waiting) Tm.mark("waiting") else Tm.Dim),
                    maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        row.context?.let { Text("$it%", style = Tm.Small.copy(fontSize = 12.sp, color = Tm.Faint), modifier = Modifier.padding(start = 8.dp)) }
    }
}

/** One host: straight to its form; several: which one first. */
@Composable
private fun NewSession(hosts: List<HostInfo>, onNew: (HostInfo) -> Unit) {
    var menu by remember { mutableStateOf(false) }
    Box {
        Row(
            Modifier.fillMaxWidth()
                .clickable(enabled = hosts.isNotEmpty(), role = Role.Button) { if (hosts.size == 1) onNew(hosts[0]) else menu = true }
                .padding(horizontal = 24.dp, vertical = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Glyphs.Plus, null, Modifier.size(20.dp), tint = if (hosts.isEmpty()) Tm.Faint else Tm.Accent)
            Text("New session", style = Tm.Body.copy(color = if (hosts.isEmpty()) Tm.Faint else Tm.Text), modifier = Modifier.padding(start = 14.dp))
        }
        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }, containerColor = Tm.Raised) {
            hosts.forEach { host ->
                DropdownMenuItem(text = { Text(host.label) }, onClick = {
                    menu = false
                    onNew(host)
                })
            }
        }
    }
}
