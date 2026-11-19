package org.sonam.server;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;

/** Brings the server back after a reboot or an app update, if the user wants that. */
public final class BootReceiver extends BroadcastReceiver {
    @Override public void onReceive(Context context, Intent intent) {
        String action = intent.getAction();
        if (!Intent.ACTION_BOOT_COMPLETED.equals(action) && !Intent.ACTION_MY_PACKAGE_REPLACED.equals(action)) return;
        if (!Prefs.startOnBoot(context) || !Prefs.wantsRunning(context)) return;
        try { ServerService.start(context); }
        catch (RuntimeException e) { /* Android refused a background start; the app starts it when opened. */ }
    }
}
