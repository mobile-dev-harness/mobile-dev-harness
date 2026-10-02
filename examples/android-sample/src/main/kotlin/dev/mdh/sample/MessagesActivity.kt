package dev.mdh.sample

import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.TextView
import androidx.activity.ComponentActivity
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView

/** A long list (100 rows) for scrolling and list folding. */
class MessagesActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_messages)
        padForSystemBars()
        val selected = findViewById<TextView>(R.id.selected)
        findViewById<RecyclerView>(R.id.messages).apply {
            layoutManager = LinearLayoutManager(this@MessagesActivity)
            adapter = MessageAdapter { n -> selected.text = "Opened Message $n" }
        }
    }
}

private class MessageAdapter(private val onOpen: (Int) -> Unit) :
    RecyclerView.Adapter<MessageAdapter.Holder>() {

    class Holder(view: View) : RecyclerView.ViewHolder(view) {
        val title: TextView = view.findViewById(R.id.title)
        val from: TextView = view.findViewById(R.id.from)
    }

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int) =
        Holder(LayoutInflater.from(parent.context).inflate(R.layout.item_message, parent, false))

    override fun getItemCount() = 100

    override fun onBindViewHolder(holder: Holder, position: Int) {
        val n = position + 1
        holder.title.text = "Message $n"
        holder.from.text = if (n % 3 == 0) "From Carol" else "From Bob"
        holder.itemView.setOnClickListener { onOpen(n) }
    }
}
