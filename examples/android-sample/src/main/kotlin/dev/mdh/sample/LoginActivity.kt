package dev.mdh.sample

import android.content.Intent
import android.os.Bundle
import android.view.View
import android.widget.Button
import android.widget.EditText
import android.widget.ProgressBar
import android.widget.TextView
import androidx.activity.ComponentActivity
import androidx.core.widget.doAfterTextChanged

/** Sign in is enabled once both fields are filled; the check takes a moment, like a network call. */
class LoginActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_login)
        padForSystemBars()
        val email = findViewById<EditText>(R.id.email)
        val password = findViewById<EditText>(R.id.password)
        val signIn = findViewById<Button>(R.id.sign_in)
        val progress = findViewById<ProgressBar>(R.id.progress)
        val error = findViewById<TextView>(R.id.error)

        val update = {
            signIn.isEnabled = email.text.isNotBlank() && password.text.isNotEmpty()
            error.visibility = View.GONE
        }
        email.doAfterTextChanged { update() }
        password.doAfterTextChanged { update() }

        signIn.setOnClickListener {
            signIn.isEnabled = false
            progress.visibility = View.VISIBLE
            signIn.postDelayed({
                progress.visibility = View.GONE
                if (email.text.toString() == EMAIL && password.text.toString() == PASSWORD) {
                    startActivity(Intent(this, MessagesActivity::class.java))
                    finish()
                } else {
                    error.visibility = View.VISIBLE
                    signIn.isEnabled = true
                }
            }, 800)
        }
    }

    companion object {
        const val EMAIL = "alice@example.com"
        const val PASSWORD = "correct-horse"
    }
}
