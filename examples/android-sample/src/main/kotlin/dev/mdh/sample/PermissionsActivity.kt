package dev.mdh.sample

import android.Manifest
import android.content.pm.PackageManager
import android.os.Bundle
import android.widget.Button
import android.widget.TextView
import androidx.activity.ComponentActivity

/** Requests a runtime permission, which shows a system dialog over the app. */
class PermissionsActivity : ComponentActivity() {
    private lateinit var status: TextView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_permissions)
        padForSystemBars()
        status = findViewById(R.id.camera_status)
        showStatus()
        findViewById<Button>(R.id.request_camera).setOnClickListener {
            requestPermissions(arrayOf(Manifest.permission.CAMERA), REQUEST_CAMERA)
        }
    }

    @Deprecated("Fine for a sample; the result contract API needs no request codes")
    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        showStatus()
    }

    private fun showStatus() {
        val granted = checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED
        status.text = if (granted) "Camera: granted" else "Camera: not granted"
    }

    private companion object {
        const val REQUEST_CAMERA = 1
    }
}
