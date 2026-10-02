package dev.mdh.helper;

import android.net.LocalServerSocket;
import android.net.LocalSocket;
import android.util.Log;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.OutputStreamWriter;
import java.io.Writer;
import java.nio.charset.StandardCharsets;

import org.json.JSONException;
import org.json.JSONObject;

/**
 * Newline-delimited JSON over an abstract Unix socket ({@code localabstract:mdh-helper}), reached
 * from the host through {@code adb forward}. Abstract sockets need no permission and no port.
 */
final class Server {
    static final String SOCKET_NAME = "mdh-helper";

    private final Commands commands;

    Server(Commands commands) {
        this.commands = commands;
    }

    void run() throws IOException {
        LocalServerSocket server = new LocalServerSocket(SOCKET_NAME);
        Log.i(HelperInstrumentation.TAG, "listening on localabstract:" + SOCKET_NAME);
        while (true) {
            LocalSocket client = server.accept();
            new Thread(() -> serve(client)).start();
        }
    }

    private void serve(LocalSocket client) {
        try (LocalSocket socket = client;
                BufferedReader in = new BufferedReader(
                        new InputStreamReader(socket.getInputStream(), StandardCharsets.UTF_8));
                Writer out = new OutputStreamWriter(socket.getOutputStream(), StandardCharsets.UTF_8)) {
            String line;
            while ((line = in.readLine()) != null) {
                out.write(handle(line));
                out.write('\n');
                out.flush();
            }
        } catch (IOException e) {
            Log.w(HelperInstrumentation.TAG, "client disconnected", e);
        }
    }

    private String handle(String line) {
        try {
            JSONObject response;
            // One UiAutomation connection is shared by all clients; serialize access to it.
            synchronized (commands) {
                response = commands.dispatch(new JSONObject(line));
            }
            return response.put("ok", true).toString();
        } catch (Exception e) {
            try {
                return new JSONObject().put("ok", false).put("error", String.valueOf(e)).toString();
            } catch (JSONException impossible) {
                return "{\"ok\":false}";
            }
        }
    }
}
