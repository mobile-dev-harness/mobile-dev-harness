package dev.mdh.sample

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.widget.Button
import androidx.activity.ComponentActivity

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)
        padForSystemBars()
        mapOf(
            R.id.open_login to LoginActivity::class.java,
            R.id.open_messages to MessagesActivity::class.java,
            R.id.open_settings to SettingsActivity::class.java,
            R.id.open_compose to ComposeActivity::class.java,
            R.id.open_web to WebActivity::class.java,
            R.id.open_troubles to TroublesActivity::class.java,
            R.id.open_permissions to PermissionsActivity::class.java,
        ).forEach { (id, screen) -> open(id, screen) }
    }

    private fun open(id: Int, screen: Class<out Activity>) {
        findViewById<Button>(id).setOnClickListener { startActivity(Intent(this, screen)) }
    }
}
