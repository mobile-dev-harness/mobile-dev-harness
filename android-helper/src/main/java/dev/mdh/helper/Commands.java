package dev.mdh.helper;

import android.app.UiAutomation;
import android.graphics.Rect;
import android.os.Build;
import android.os.Bundle;
import android.os.SystemClock;
import android.view.InputDevice;
import android.view.InputEvent;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityWindowInfo;

import java.util.concurrent.TimeoutException;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

/** Request handlers. Field names of the tree match {@code mdh_core::ui::RawNode}. */
final class Commands {
    /** Bump together with versionCode and HELPER_VERSION_CODE on the host. */
    static final int VERSION_CODE = 2;

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
                return new JSONObject()
                        .put("roots", new JSONArray().put(node(root())))
                        .put("windows", windows());
            case "tap":
                swipe(request.getInt("x"), request.getInt("y"), request.getInt("x"), request.getInt("y"), 0);
                return new JSONObject();
            case "swipe":
                swipe(request.getInt("x1"), request.getInt("y1"), request.getInt("x2"), request.getInt("y2"),
                        request.getLong("duration_ms"));
                return new JSONObject();
            case "key":
                key(request.getString("key"));
                return new JSONObject();
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

    /**
     * Injects a touch gesture from (x1, y1) to (x2, y2). A tap is a zero-length, zero-duration
     * gesture; a long press is zero-length with a duration.
     */
    private void swipe(int x1, int y1, int x2, int y2, long durationMs) {
        long down = SystemClock.uptimeMillis();
        inject(touch(down, down, MotionEvent.ACTION_DOWN, x1, y1));
        long steps = Math.max(1, durationMs / 10);
        for (long i = 1; i < steps; i++) {
            long at = down + durationMs * i / steps;
            SystemClock.sleep(Math.max(0, at - SystemClock.uptimeMillis()));
            float x = x1 + (x2 - x1) * (float) i / steps;
            float y = y1 + (y2 - y1) * (float) i / steps;
            inject(touch(down, SystemClock.uptimeMillis(), MotionEvent.ACTION_MOVE, x, y));
        }
        SystemClock.sleep(Math.max(0, down + durationMs - SystemClock.uptimeMillis()));
        inject(touch(down, SystemClock.uptimeMillis(), MotionEvent.ACTION_UP, x2, y2));
    }

    private static MotionEvent touch(long downTime, long eventTime, int action, float x, float y) {
        MotionEvent event = MotionEvent.obtain(downTime, eventTime, action, x, y, 0);
        event.setSource(InputDevice.SOURCE_TOUCHSCREEN);
        return event;
    }

    /** {@code name} is a key code name without the prefix, e.g. {@code BACK} or {@code ENTER}. */
    private void key(String name) {
        int code = KeyEvent.keyCodeFromString("KEYCODE_" + name);
        if (code == KeyEvent.KEYCODE_UNKNOWN) {
            throw new IllegalArgumentException("unknown key " + name);
        }
        long now = SystemClock.uptimeMillis();
        inject(new KeyEvent(now, now, KeyEvent.ACTION_DOWN, code, 0));
        inject(new KeyEvent(now, now, KeyEvent.ACTION_UP, code, 0));
    }

    /**
     * Asynchronous: returns once the event is queued. Synchronous injection waits until the target
     * window has handled it, which was observed to take 0.4-1.7 s while the app animates; settling
     * is the host's job (wait_stable). Events in one queue keep their order.
     */
    private void inject(InputEvent event) {
        if (!automation.injectInputEvent(event, false)) {
            throw new IllegalStateException("input injection rejected");
        }
    }

    /** On-screen windows, top-most first: reveals the keyboard and system dialogs over the app. */
    private JSONArray windows() throws JSONException {
        JSONArray out = new JSONArray();
        for (AccessibilityWindowInfo w : automation.getWindows()) {
            JSONObject o = new JSONObject()
                    .put("kind", windowKind(w.getType()))
                    .put("active", w.isActive())
                    .put("focused", w.isFocused());
            putText(o, "title", w.getTitle());
            AccessibilityNodeInfo root = w.getRoot();
            if (root != null) {
                putText(o, "package", root.getPackageName());
            }
            Rect r = new Rect();
            w.getBoundsInScreen(r);
            o.put("bounds", rect(r));
            out.put(o);
        }
        return out;
    }

    private static String windowKind(int type) {
        switch (type) {
            case AccessibilityWindowInfo.TYPE_APPLICATION:
                return "application";
            case AccessibilityWindowInfo.TYPE_INPUT_METHOD:
                return "input_method";
            case AccessibilityWindowInfo.TYPE_SYSTEM:
                return "system";
            case AccessibilityWindowInfo.TYPE_ACCESSIBILITY_OVERLAY:
                return "accessibility_overlay";
            default:
                return "other";
        }
    }

    private static JSONObject rect(Rect r) throws JSONException {
        return new JSONObject()
                .put("left", r.left).put("top", r.top).put("right", r.right).put("bottom", r.bottom);
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
        o.put("bounds", rect(r));

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
