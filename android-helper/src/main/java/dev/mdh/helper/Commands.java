package dev.mdh.helper;

import android.app.UiAutomation;
import android.graphics.Rect;
import android.os.Build;
import android.os.Bundle;
import android.os.SystemClock;
import android.view.accessibility.AccessibilityNodeInfo;

import java.util.concurrent.TimeoutException;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

/** Request handlers. Field names of the tree match {@code mdh_core::ui::RawNode}. */
final class Commands {
    /** Bump together with versionCode and HELPER_VERSION_CODE on the host. */
    static final int VERSION_CODE = 1;

    private static final long ROOT_RETRY_MS = 2000;

    private final UiAutomation automation;

    Commands(UiAutomation automation) {
        this.automation = automation;
    }

    JSONObject dispatch(JSONObject request) throws Exception {
        String cmd = request.getString("cmd");
        switch (cmd) {
            case "ping":
                return new JSONObject().put("version_code", VERSION_CODE).put("sdk", Build.VERSION.SDK_INT);
            case "tree":
                return new JSONObject().put("roots", new JSONArray().put(node(root())));
            case "wait_idle":
                return waitIdle(request.optLong("quiet_ms", 300), request.optLong("timeout_ms", 5000));
            case "set_text":
                return setText(request.getString("text"));
            default:
                throw new IllegalArgumentException("unknown cmd " + cmd);
        }
    }

    /** The active window's root; briefly null during window transitions, so retry. */
    private AccessibilityNodeInfo root() {
        long deadline = SystemClock.uptimeMillis() + ROOT_RETRY_MS;
        while (true) {
            AccessibilityNodeInfo root = automation.getRootInActiveWindow();
            if (root != null) {
                return root;
            }
            if (SystemClock.uptimeMillis() > deadline) {
                throw new IllegalStateException("no active window");
            }
            SystemClock.sleep(50);
        }
    }

    private JSONObject waitIdle(long quietMs, long timeoutMs) throws JSONException {
        long started = SystemClock.uptimeMillis();
        boolean idle = true;
        try {
            automation.waitForIdle(quietMs, timeoutMs);
        } catch (TimeoutException e) {
            idle = false;
        }
        return new JSONObject().put("idle", idle).put("waited_ms", SystemClock.uptimeMillis() - started);
    }

    /** Replaces the content of the focused input. Works for any Unicode text, unlike `input text`. */
    private JSONObject setText(String text) throws JSONException {
        AccessibilityNodeInfo focused = root().findFocus(AccessibilityNodeInfo.FOCUS_INPUT);
        if (focused == null || !focused.isEditable()) {
            throw new IllegalStateException("no focused text field");
        }
        Bundle args = new Bundle();
        args.putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, text);
        if (!focused.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)) {
            throw new IllegalStateException("the focused field rejected the text");
        }
        return new JSONObject();
    }

    private static JSONObject node(AccessibilityNodeInfo n) throws JSONException {
        JSONObject o = new JSONObject();
        o.put("class", String.valueOf(n.getClassName()));
        putText(o, "package", n.getPackageName());
        putText(o, "resource_id", n.getViewIdResourceName());
        putText(o, "text", n.getText());
        putText(o, "desc", n.getContentDescription());
        putText(o, "hint", n.getHintText());

        Rect r = new Rect();
        n.getBoundsInScreen(r);
        o.put("bounds", new JSONObject()
                .put("left", r.left).put("top", r.top).put("right", r.right).put("bottom", r.bottom));

        o.put("flags", new JSONObject()
                .put("clickable", n.isClickable())
                .put("long_clickable", n.isLongClickable())
                .put("checkable", n.isCheckable())
                .put("checked", n.isChecked())
                .put("enabled", n.isEnabled())
                .put("focusable", n.isFocusable())
                .put("focused", n.isFocused())
                .put("scrollable", n.isScrollable())
                .put("selected", n.isSelected())
                .put("password", n.isPassword()));

        JSONArray children = new JSONArray();
        for (int i = 0; i < n.getChildCount(); i++) {
            AccessibilityNodeInfo child = n.getChild(i);
            // Like `uiautomator dump`, skip what the user can't see.
            if (child != null && child.isVisibleToUser()) {
                children.put(node(child));
            }
        }
        o.put("children", children);
        return o;
    }

    private static void putText(JSONObject o, String key, CharSequence value) throws JSONException {
        if (value != null && value.length() > 0) {
            o.put(key, value.toString());
        }
    }
}
