package dev.mdh.sample

import android.annotation.SuppressLint
import android.os.Bundle
import android.webkit.WebView
import androidx.activity.ComponentActivity

/** A local page in a WebView: its content may not be visible in the accessibility tree. */
class WebActivity : ComponentActivity() {
    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val web = WebView(this)
        web.settings.javaScriptEnabled = true
        web.loadUrl("file:///android_asset/page.html")
        setContentView(web)
        padForSystemBars()
    }
}
