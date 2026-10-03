package dev.mdh.sample

import android.os.Bundle
import android.os.Process
import android.util.Log
import android.view.View
import android.widget.Button
import android.widget.ProgressBar
import android.widget.TextView
import androidx.activity.ComponentActivity

/** Buttons that misbehave on purpose, so crash, ANR and log reporting can be checked for real. */
class TroublesActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_troubles)
        padForSystemBars()
        val status = findViewById<TextView>(R.id.status)
        val progress = findViewById<ProgressBar>(R.id.progress)

        click(R.id.crash_java) { Checkout().pay(cartId = "") }
        // SIGSEGV sent to ourselves goes through the native crash handler and crash_dump, like a
        // real native crash, without needing any native code.
        click(R.id.crash_native) { Process.sendSignal(Process.myPid(), SIGSEGV) }
        // Blocks the main thread; the next input event makes the system report an ANR.
        click(R.id.freeze) {
            status.text = "Frozen"
            Thread.sleep(12_000)
        }
        click(R.id.slow) {
            status.text = "Loading…"
            progress.visibility = View.VISIBLE
            progress.postDelayed({
                progress.visibility = View.GONE
                status.text = "Loaded 3 items"
            }, 2_000)
        }
        click(R.id.overlap) { startActivity(android.content.Intent(this, OverlapActivity::class.java)) }
        findViewById<android.widget.ImageButton>(R.id.share).setOnClickListener { status.text = "Shared" }
        click(R.id.log_errors) {
            repeat(3) { Log.e("SampleNetwork", "timeout after 10000 ms") }
            Log.w("SampleAuth", "token cache miss")
            status.text = "Logged"
        }
    }

    private fun click(id: Int, action: () -> Unit) {
        findViewById<Button>(id).setOnClickListener { action() }
    }

    private companion object {
        const val SIGSEGV = 11
    }
}

/** App code in the stack trace of the Java crash, with a cause. */
class Checkout {
    fun pay(cartId: String) {
        try {
            requireCart(cartId)
        } catch (e: IllegalArgumentException) {
            throw IllegalStateException("Sample crash: could not pay for the cart", e)
        }
    }

    private fun requireCart(cartId: String) {
        require(cartId.isNotBlank()) { "cart id must not be blank" }
    }
}
