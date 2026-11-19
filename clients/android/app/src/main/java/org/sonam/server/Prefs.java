package org.sonam.server;

import android.content.Context;
import android.content.SharedPreferences;
import android.os.Build;
import android.os.Environment;
import android.os.StatFs;
import java.io.File;
import java.util.ArrayList;
import java.util.List;

/** Small persisted choices: start on boot and where the library lives. */
final class Prefs {
    static final int PORT = 8096;
    private Prefs() {}

    private static SharedPreferences p(Context c) { return c.getSharedPreferences("server", Context.MODE_PRIVATE); }
    static boolean startOnBoot(Context c) { return p(c).getBoolean("boot", true); }
    static void setStartOnBoot(Context c, boolean v) { p(c).edit().putBoolean("boot", v).apply(); }
    static boolean wantsRunning(Context c) { return p(c).getBoolean("running", true); }
    static void setWantsRunning(Context c, boolean v) { p(c).edit().putBoolean("running", v).apply(); }

    static final class Place {
        final String label; final File dir; final boolean needsPermission;
        Place(String label, File dir, boolean needsPermission) { this.label = label; this.dir = dir; this.needsPermission = needsPermission; }
        long free() {
            try { File d = dir.exists() ? dir : dir.getParentFile(); return d == null ? 0 : new StatFs(d.getPath()).getAvailableBytes(); }
            catch (RuntimeException e) { return 0; }
        }
    }

    /** Internal app storage (fast, private), SD cards (app folder) and an optional public folder. */
    static List<Place> places(Context c) {
        List<Place> out = new ArrayList<>();
        out.add(new Place("Memória interna do app", new File(c.getFilesDir(), "library"), false));
        File[] external = c.getExternalFilesDirs(null);
        for (int i = 1; i < external.length; i++) {
            if (external[i] != null) out.add(new Place("Cartão SD (" + (i) + ")", new File(external[i], "library"), false));
        }
        out.add(new Place("Pasta pública Sonam (continua após desinstalar)",
                new File(Environment.getExternalStorageDirectory(), "Sonam"), true));
        return out;
    }

    static File dataDir(Context c) {
        String saved = p(c).getString("data", null);
        return saved != null ? new File(saved) : places(c).get(0).dir;
    }
    static void setDataDir(Context c, File dir) { p(c).edit().putString("data", dir.getAbsolutePath()).apply(); }

    static boolean hasAllFilesAccess() {
        return Build.VERSION.SDK_INT >= 30 && Environment.isExternalStorageManager();
    }
}
