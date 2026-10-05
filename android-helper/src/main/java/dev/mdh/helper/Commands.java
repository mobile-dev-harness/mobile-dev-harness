package dev.mdh.helper;

import android.app.UiAutomation;
import android.graphics.Rect;
import android.graphics.Region;
import android.os.Build;
import android.os.Bundle;
import android.os.SystemClock;
import android.view.InputDevice;
import android.view.InputEvent;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityWindowInfo;

import java.util.List;
import java.util.concurrent.TimeoutException;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

/** Request handlers. Field names of the tree match {@code mdh_core::ui::RawNode}. */
final class Commands {
    /** Bump together with versionCode and HELPER_VERSION_CODE on the host. */
    static final int VERSION_CODE = 6;

    private static final long ROOT_RETRY_MS = 500;

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
                clearCache();
                AccessibilityNodeInfo root = root();
                List<AccessibilityWindowInfo> windows = automation.getWindows();
                return new JSONObject()
                        .put("roots", new JSONArray().put(node(root, systemCovers(root, windows))))
                        .put("windows", windows(windows));
            case "tap":
                swipe(request.getInt("x"), request.getInt("y"), request.getInt("x"), request.getInt("y"), 0, 0);
                return new JSONObject();
            case "swipe":
                swipe(request.getInt("x1"), request.getInt("y1"), request.getInt("x2"), request.getInt("y2"),
                        request.getLong("duration_ms"), request.optLong("hold_ms", 0));
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

    /**
     * Drops what the connection has cached of the screen, so a read sees the screen as it is. The
     * connection lives as long as the helper and events keep its cache current; after a Compose
     * navigation within one window (Now in Android's list-detail panes) it kept returning the previous
     * screen until the helper restarted.
     */
    private void clearCache() {
        if (Build.VERSION.SDK_INT >= 34) {
            automation.clearCache();
            return;
        }
        // Before API 34 the cache is the process-wide interaction client's, reachable only by
        // reflection; without it a read may be stale, as before.
        try {
            Class<?> client = Class.forName("android.view.accessibility.AccessibilityInteractionClient");
            Object instance = client.getMethod("getInstance").invoke(null);
            client.getMethod("clearCache").invoke(instance);
        } catch (ReflectiveOperationException | RuntimeException e) {
            // Not available on this version.
        }
    }

    /**
     * The active window's root. There is briefly no active window during transitions and while an
     * app dies (observed), so retry, then fall back to the top-most window that has content.
     */
    private AccessibilityNodeInfo root() {
        long deadline = SystemClock.uptimeMillis() + ROOT_RETRY_MS;
        while (true) {
            AccessibilityNodeInfo root = automation.getRootInActiveWindow();
            if (root != null) {
                return root;
            }
            if (SystemClock.uptimeMillis() > deadline) {
                break;
            }
            SystemClock.sleep(50);
        }
        for (AccessibilityWindowInfo w : automation.getWindows()) {
            AccessibilityNodeInfo root = w.getRoot();
            if (root != null && w.getType() == AccessibilityWindowInfo.TYPE_APPLICATION) {
                return root;
            }
        }
        throw new IllegalStateException("no window with content");
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
     * gesture; a long press is zero-length with a duration. Holding still at the end for
     * {@code holdMs} releases with no velocity, so lists stop where the finger stopped instead of
     * flinging past the content (used for scrolling).
     */
    private void swipe(int x1, int y1, int x2, int y2, long durationMs, long holdMs) {
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
        for (long held = 0; held < holdMs; held += 10) {
            inject(touch(down, SystemClock.uptimeMillis(), MotionEvent.ACTION_MOVE, x2, y2));
            SystemClock.sleep(10);
        }
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
    private static JSONArray windows(List<AccessibilityWindowInfo> windows) throws JSONException {
        JSONArray out = new JSONArray();
        for (AccessibilityWindowInfo w : windows) {
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

    /**
     * The area of the system's windows over {@code root}'s window: status and navigation bars and
     * the keyboard, the windows the host treats as obstructions ({@code ScreenInfo::new}).
     */
    private static Region systemCovers(AccessibilityNodeInfo root, List<AccessibilityWindowInfo> windows) {
        Region covers = new Region();
        int layer = Integer.MAX_VALUE;
        for (AccessibilityWindowInfo w : windows) {
            if (w.getId() == root.getWindowId()) {
                layer = w.getLayer();
            }
        }
        Rect r = new Rect();
        for (AccessibilityWindowInfo w : windows) {
            int type = w.getType();
            boolean system = type == AccessibilityWindowInfo.TYPE_SYSTEM
                    || type == AccessibilityWindowInfo.TYPE_INPUT_METHOD;
            if (system && w.getLayer() > layer && !w.isActive() && !w.isFocused()) {
                w.getBoundsInScreen(r);
                covers.op(r, Region.Op.UNION);
            }
        }
        return covers;
    }

    /**
     * Whether {@code n} lies entirely under {@code covers}. Listing windows
     * (FLAG_RETRIEVE_INTERACTIVE_WINDOWS) makes the system report such a node as not visible to the
     * user whatever the app shows there, which `uiautomator dump` doesn't do: an app that draws
     * its back button under the status bar lost it from the tree (observed with Now in Android)
     * instead of having it reported as obscured.
     */
    private static boolean covered(AccessibilityNodeInfo n, Region covers) {
        Rect r = new Rect();
        n.getBoundsInScreen(r);
        if (r.isEmpty() || covers.isEmpty()) {
            return false;
        }
        Region rest = new Region(r);
        return !rest.op(covers, Region.Op.DIFFERENCE);
    }

    private static JSONObject node(AccessibilityNodeInfo n, Region covers) throws JSONException {
        JSONObject o = new JSONObject();
        o.put("class", String.valueOf(n.getClassName()));
        putText(o, "package", n.getPackageName());
        putText(o, "resource_id", n.getViewIdResourceName());
        putText(o, "text", n.getText());
        putText(o, "desc", n.getContentDescription());
        putText(o, "hint", n.getHintText());
        // Compose and other toolkits announce roles here when the class name is a generic View
        // (e.g. a toggleable row with Role.Switch).
        Bundle extras = n.getExtras();
        if (extras != null) {
            putText(o, "role_description", extras.getCharSequence("AccessibilityNodeInfo.roleDescription"));
        }

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
            // Like `uiautomator dump`, skip what the app doesn't show; what it shows under a system
            // window stays, for the host to mark as obscured.
            if (child != null && (child.isVisibleToUser() || covered(child, covers))) {
                children.put(node(child, covers));
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
