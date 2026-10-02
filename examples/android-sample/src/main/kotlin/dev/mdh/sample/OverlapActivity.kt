package dev.mdh.sample

import android.os.Bundle
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import androidx.activity.ComponentActivity

/**
 * A deliberate edge-to-edge bug: an action bar theme without inset handling, so the first button is
 * drawn under the action bar. Agents should be told it is obscured instead of tapping the bar.
 */
class OverlapActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val status = TextView(this).apply { text = "Nothing tapped" }
        val layout = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(Button(context).apply {
                text = "Hidden button"
                setOnClickListener { status.text = "Tapped the hidden button" }
            })
            // Pushes the next button below the action bar (status bar + bar ≈ 330 px on a phone).
            addView(android.view.View(context), LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, 400))
            addView(Button(context).apply {
                text = "Visible button"
                setOnClickListener { status.text = "Tapped the visible button" }
            })
            addView(status)
        }
        setContentView(layout)
    }
}
