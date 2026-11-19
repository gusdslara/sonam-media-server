package org.sonam.server;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.net.ConnectivityManager;
import android.net.LinkAddress;
import android.net.LinkProperties;
import android.net.Network;
import android.net.wifi.WifiManager;
import android.os.Build;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.os.PowerManager;
import java.io.File;
import java.net.Inet4Address;
import java.util.ArrayList;
import java.util.List;

/**
 * Keeps the phone working as a home server: foreground service (survives the
 * app being closed), partial wake lock (CPU keeps serving with the screen off),
 * Wi-Fi lock (radio does not doze) and multicast lock (".local" name).
 */
public final class ServerService extends Service {
    static final String ACTION_STOP = "org.sonam.server.STOP";
    private static final String CHANNEL = "server";
    private static final int NOTIFICATION = 1;

    /** Read by the activity; the service and activity share one process. */
    static volatile String state = "off"; // off | starting | on | error
    static volatile String error = null;
    /** Name announced on the network (mDNS), e.g. "sonam" -> sonam.local. */
    static volatile String name = null;

    private PowerManager.WakeLock wake;
    private WifiManager.WifiLock wifi;
    private WifiManager.MulticastLock multicast;
    private ConnectivityManager.NetworkCallback networkCallback;
    private final Handler main = new Handler(Looper.getMainLooper());

    static void start(Context c) {
        Prefs.setWantsRunning(c, true);
        Intent i = new Intent(c, ServerService.class);
        if (Build.VERSION.SDK_INT >= 26) c.startForegroundService(i); else c.startService(i);
    }

    static void stop(Context c) {
        Prefs.setWantsRunning(c, false);
        c.startService(new Intent(c, ServerService.class).setAction(ACTION_STOP));
    }

    @Override public IBinder onBind(Intent intent) { return null; }

    @Override public int onStartCommand(Intent intent, int flags, int startId) {
        if (intent != null && ACTION_STOP.equals(intent.getAction())) {
            // Also from the notification button: the app must not restart it.
            Prefs.setWantsRunning(this, false);
            state = "stopping";
            goForeground("Desligando…");
            stopSelf();
            return START_NOT_STICKY;
        }
        goForeground(state.equals("on") ? onlineText() : "Iniciando o servidor…");
        if (!"on".equals(state) && !"starting".equals(state)) launch();
        return START_STICKY;
    }

    private void launch() {
        state = "starting";
        error = null;
        acquireLocks();
        watchNetwork();
        final File data = Prefs.dataDir(this);
        new Thread(() -> {
            String failure;
            if (!data.exists() && !data.mkdirs()) failure = "Não foi possível criar a pasta " + data;
            else failure = ServerBridge.start(data.getAbsolutePath(), Prefs.PORT);
            if (failure == null) name = fetchName();
            final String result = failure;
            main.post(() -> {
                if (result == null) { state = "on"; goForeground(onlineText()); }
                else { state = "error"; error = result; goForeground("Erro: " + result); }
            });
        }, "sonam-start").start();
    }

    private void acquireLocks() {
        if (wake == null) {
            PowerManager pm = (PowerManager) getSystemService(POWER_SERVICE);
            wake = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "Sonam:server");
            wake.setReferenceCounted(false);
            wake.acquire();
        }
        WifiManager wm = (WifiManager) getApplicationContext().getSystemService(WIFI_SERVICE);
        if (wm == null) return;
        if (wifi == null) {
            @SuppressWarnings("deprecation") int mode = WifiManager.WIFI_MODE_FULL_HIGH_PERF;
            wifi = wm.createWifiLock(mode, "Sonam:wifi");
            wifi.setReferenceCounted(false);
            wifi.acquire();
        }
        if (multicast == null) {
            multicast = wm.createMulticastLock("Sonam:mdns");
            multicast.setReferenceCounted(false);
            multicast.acquire();
        }
    }

    /** Refresh the address in the notification when Wi-Fi changes. */
    private void watchNetwork() {
        if (networkCallback != null) return;
        ConnectivityManager cm = (ConnectivityManager) getSystemService(CONNECTIVITY_SERVICE);
        networkCallback = new ConnectivityManager.NetworkCallback() {
            @Override public void onLinkPropertiesChanged(Network n, LinkProperties lp) { refresh(); }
            @Override public void onLost(Network n) { refresh(); }
            private void refresh() { main.post(() -> { if ("on".equals(state)) goForeground(onlineText()); }); }
        };
        try { cm.registerDefaultNetworkCallback(networkCallback); } catch (RuntimeException ignored) { networkCallback = null; }
    }

    /** Private IPv4 addresses of this phone (Wi-Fi/Ethernet), best first. */
    static List<String> lanAddresses(Context c) {
        List<String> out = new ArrayList<>();
        ConnectivityManager cm = (ConnectivityManager) c.getSystemService(CONNECTIVITY_SERVICE);
        for (Network n : cm.getAllNetworks()) {
            LinkProperties lp = cm.getLinkProperties(n);
            if (lp == null) continue;
            for (LinkAddress la : lp.getLinkAddresses()) {
                if (la.getAddress() instanceof Inet4Address && la.getAddress().isSiteLocalAddress()) {
                    String ip = la.getAddress().getHostAddress();
                    if (ip.startsWith("192.168.")) out.add(0, ip); else out.add(ip);
                }
            }
        }
        return out;
    }

    static String bestUrl(Context c) {
        List<String> ips = lanAddresses(c);
        return ips.isEmpty() ? null : "http://" + ips.get(0) + ":" + Prefs.PORT;
    }

    /** Friendly address (mDNS). Works on Windows, macOS, iPhone and Linux. */
    static String nameUrl() {
        return name == null ? null : "http://" + name + ".local:" + Prefs.PORT;
    }

    /** Reads the announced name from the local server (/api/status). */
    static String fetchName() {
        for (int attempt = 0; attempt < 20; attempt++) {
            try {
                java.net.HttpURLConnection c = (java.net.HttpURLConnection)
                        new java.net.URL("http://127.0.0.1:" + Prefs.PORT + "/api/status").openConnection();
                c.setConnectTimeout(1000);
                c.setReadTimeout(1000);
                if (c.getResponseCode() == 200) {
                    java.io.InputStream in = c.getInputStream();
                    String body = new java.util.Scanner(in, "UTF-8").useDelimiter("\\A").next();
                    c.disconnect();
                    java.util.regex.Matcher m = java.util.regex.Pattern.compile("\"name\":\"([a-z0-9-]+)\"").matcher(body);
                    return m.find() ? m.group(1) : null;
                }
                c.disconnect();
            } catch (Exception ignored) {}
            try { Thread.sleep(150); } catch (InterruptedException e) { return null; }
        }
        return null;
    }

    private String onlineText() {
        String ip = bestUrl(this), friendly = nameUrl();
        if (ip == null) return "Ligado · conecte o celular ao Wi-Fi de casa";
        return friendly == null ? "Ligado · " + ip : "Ligado · " + friendly + "\nou " + ip;
    }

    private void goForeground(String text) {
        NotificationManager nm = (NotificationManager) getSystemService(NOTIFICATION_SERVICE);
        if (Build.VERSION.SDK_INT >= 26 && nm.getNotificationChannel(CHANNEL) == null) {
            NotificationChannel ch = new NotificationChannel(CHANNEL, "Servidor Sonam", NotificationManager.IMPORTANCE_LOW);
            ch.setDescription("Mostra que o celular está servindo sua biblioteca na rede");
            ch.setShowBadge(false);
            nm.createNotificationChannel(ch);
        }
        int immutable = Build.VERSION.SDK_INT >= 23 ? PendingIntent.FLAG_IMMUTABLE : 0;
        PendingIntent open = PendingIntent.getActivity(this, 0,
                new Intent(this, MainActivity.class).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP), immutable);
        PendingIntent stop = PendingIntent.getService(this, 1,
                new Intent(this, ServerService.class).setAction(ACTION_STOP), immutable);
        Notification.Builder b = Build.VERSION.SDK_INT >= 26 ? new Notification.Builder(this, CHANNEL) : new Notification.Builder(this);
        b.setSmallIcon(R.drawable.ic_stat_server)
                .setContentTitle("Sonam")
                .setContentText(text)
                .setStyle(new Notification.BigTextStyle().bigText(text))
                .setOngoing(true)
                .setShowWhen(false)
                .setContentIntent(open)
                .addAction(new Notification.Action.Builder(null, "Desligar", stop).build());
        if (Build.VERSION.SDK_INT >= 31) b.setForegroundServiceBehavior(Notification.FOREGROUND_SERVICE_IMMEDIATE);
        Notification n = b.build();
        if (Build.VERSION.SDK_INT >= 34) startForeground(NOTIFICATION, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE);
        else startForeground(NOTIFICATION, n);
    }

    @Override public void onTaskRemoved(Intent rootIntent) {
        // Swiping the app away keeps the server running; the notification stays.
        super.onTaskRemoved(rootIntent);
    }

    @Override public void onDestroy() {
        state = "off";
        if (networkCallback != null) {
            try { ((ConnectivityManager) getSystemService(CONNECTIVITY_SERVICE)).unregisterNetworkCallback(networkCallback); }
            catch (RuntimeException ignored) {}
        }
        Thread t = new Thread(ServerBridge::stop, "sonam-stop");
        t.start();
        try { t.join(6000); } catch (InterruptedException ignored) {}
        if (multicast != null && multicast.isHeld()) multicast.release();
        if (wifi != null && wifi.isHeld()) wifi.release();
        if (wake != null && wake.isHeld()) wake.release();
        super.onDestroy();
    }
}
