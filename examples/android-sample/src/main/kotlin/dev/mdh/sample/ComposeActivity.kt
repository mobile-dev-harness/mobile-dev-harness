package dev.mdh.sample

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.unit.dp

/**
 * Compose exposes semantics nodes, not views. Test tags are published as resource ids, the way many
 * Compose apps make themselves testable. One canvas deliberately has no semantics at all.
 */
class ComposeActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent { MaterialTheme { SampleScreen() } }
    }
}

@Composable
private fun SampleScreen() {
    var name by remember { mutableStateOf("") }
    var subscribed by remember { mutableStateOf(false) }
    var result by remember { mutableStateOf<String?>(null) }

    Column(
        Modifier.fillMaxSize()
            .safeDrawingPadding()
            .semantics { testTagsAsResourceId = true }
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text("Compose", style = MaterialTheme.typography.headlineSmall)
        OutlinedTextField(
            value = name,
            onValueChange = { name = it },
            label = { Text("Name") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth().testTag("name_field"),
        )
        // Echoes the input, so Unicode typing can be checked on screen.
        Text(if (name.isBlank()) "Hello, stranger" else "Hello, $name", Modifier.testTag("greeting"))
        Row(
            Modifier.fillMaxWidth()
                .toggleable(value = subscribed, role = Role.Switch, onValueChange = { subscribed = it })
                .padding(vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("Subscribe", Modifier.weight(1f))
            Switch(checked = subscribed, onCheckedChange = null)
        }
        Button(
            onClick = { result = "Saved $name" },
            enabled = name.isNotBlank(),
            modifier = Modifier.testTag("save"),
        ) { Text("Save") }
        result?.let { Text(it, Modifier.testTag("result")) }

        Text("Signature")
        // No semantics: invisible to accessibility, only a screenshot shows what's drawn here.
        Canvas(Modifier.fillMaxWidth().height(160.dp).border(1.dp, Color.Gray)) {
            drawLine(Color.Blue, Offset(40f, size.height * 0.7f), Offset(size.width - 40f, size.height * 0.3f), strokeWidth = 8f)
        }
        Canvas(Modifier.fillMaxWidth().height(80.dp).semantics { contentDescription = "Sales chart, rising" }) {
            drawLine(Color.Red, Offset(0f, size.height), Offset(size.width, 0f), strokeWidth = 6f)
        }

        LazyColumn(Modifier.fillMaxWidth().weight(1f).testTag("rows")) {
            items(30) { i ->
                Text(
                    "Row ${i + 1}",
                    Modifier.fillMaxWidth().clickable { result = "Picked row ${i + 1}" }.padding(12.dp),
                )
            }
        }
    }
}
