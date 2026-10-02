package dev.mdh.helper;

import android.accessibilityservice.AccessibilityServiceInfo;
import android.app.Instrumentation;
import android.app.UiAutomation;
import android.os.Bundle;
import android.util.Log;

/**
 * Entry point. Never finishes, so the UiAutomation connection stays warm across host invocations.
 *
 * <p>The UiAutomation connection is hosted by the {@code am} process, so it must be started with
 * {@code -w} and that process must outlive the adb session: the host runs
 * {@code nohup am instrument -w ... &} on the device.
 */
public final class HelperInstrumentation extends Instrumentation {
    static final String TAG = "mdh-helper";

    @Override
    public void onCreate(Bundle arguments) {
        super.onCreate(arguments);
        start(); // runs onStart() on a new thread
    }

    @Override
    public void onStart() {
        UiAutomation automation =
                getUiAutomation(UiAutomation.FLAG_DONT_SUPPRESS_ACCESSIBILITY_SERVICES);
        if (automation == null) {
            Log.e(TAG, "no UiAutomation connection; was `am instrument` started with -w?");
            finish(1, new Bundle());
            return;
        }
        AccessibilityServiceInfo info = automation.getServiceInfo();
        // Same view of the hierarchy as `uiautomator dump`: unimportant views and resource ids included.
        info.flags |= AccessibilityServiceInfo.FLAG_INCLUDE_NOT_IMPORTANT_VIEWS
                | AccessibilityServiceInfo.FLAG_REPORT_VIEW_IDS;
        automation.setServiceInfo(info);
        try {
            new Server(new Commands(automation)).run();
        } catch (Exception e) {
            Log.e(TAG, "server stopped", e);
            finish(1, new Bundle());
        }
    }
}
