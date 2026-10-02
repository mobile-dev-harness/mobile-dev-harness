package dev.mdh.sample

import android.os.Bundle
import android.view.View
import android.widget.LinearLayout
import android.widget.Switch
import android.widget.TextView
import androidx.activity.ComponentActivity

/** Rows with switches; Nearby share depends on Bluetooth, so one toggle changes another row. */
class SettingsActivity : ComponentActivity() {
    private lateinit var nearby: Row

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_settings)
        padForSystemBars()
        val rows = findViewById<LinearLayout>(R.id.rows)

        row(rows, "Wi-Fi", initial = true) { on -> if (on) "Connected" else "Off" }
        row(rows, "Bluetooth", initial = false) { on ->
            // Called once while the row is built, before the Nearby share row exists.
            if (::nearby.isInitialized) nearby.setEnabled(on, reason = "Needs Bluetooth")
            if (on) "On" else "Off"
        }
        nearby = row(rows, "Nearby share", initial = false) { on -> if (on) "Visible to contacts" else "Hidden" }
        nearby.setEnabled(false, reason = "Needs Bluetooth")
        row(rows, "Dark mode", initial = false) { on -> if (on) "On" else "Off" }
        row(rows, "Developer options", initial = false) { "Unavailable" }.setEnabled(false, reason = "Unavailable")
    }

    private fun row(parent: LinearLayout, title: String, initial: Boolean, summary: (Boolean) -> String): Row {
        val view = layoutInflater.inflate(R.layout.item_setting, parent, false)
        parent.addView(view)
        return Row(view, title, initial, summary)
    }

    private class Row(val view: View, title: String, initial: Boolean, private val summary: (Boolean) -> String) {
        private val toggle: Switch = view.findViewById(R.id.toggle)
        private val summaryView: TextView = view.findViewById(R.id.summary)

        init {
            view.findViewById<TextView>(R.id.title).text = title
            toggle.isChecked = initial
            summaryView.text = summary(initial)
            toggle.setOnCheckedChangeListener { _, on -> summaryView.text = summary(on) }
            view.setOnClickListener { toggle.toggle() }
        }

        fun setEnabled(enabled: Boolean, reason: String) {
            view.isEnabled = enabled
            toggle.isEnabled = enabled
            if (!enabled) toggle.isChecked = false
            summaryView.text = if (enabled) summary(toggle.isChecked) else reason
        }
    }
}
