package org.sonam.server;

import android.Manifest;
import android.app.Activity;
import android.app.AlertDialog;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.PowerManager;
import android.provider.Settings;
import android.text.format.Formatter;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.WindowManager;
import android.webkit.CookieManager;
import android.webkit.ValueCallback;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceRequest;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.Button;
import android.widget.FrameLayout;
import android.widget.ImageButton;
import android.widget.LinearLayout;
import android.widget.PopupMenu;
import android.widget.ProgressBar;
import android.widget.TextView;
import android.widget.Toast;
import java.io.File;
import java.net.HttpURLConnection;
import java.net.URL;
import java.util.List;

/**
 * The phone is both server and screen: the web interface runs inside this app
 * (no separate browser), while {@link ServerService} keeps serving in the
 * background for TVs, PCs and the network drive.
 */
public final class MainActivity extends Activity {
    private static final String ORIGIN = "http://127.0.0.1:" + Prefs.PORT;
    private static final int PICK_FILES = 10, ASK_NOTIFICATIONS = 11, ASK_STORAGE = 12;
    private static final int DARK = Color.rgb(23, 37, 30), GREEN = Color.rgb(53, 93, 73), PAPER = Color.rgb(248, 249, 246);

    private final Handler main = new Handler(Looper.getMainLooper());
    private FrameLayout content;
    private LinearLayout root, bar, panel;
    private TextView status, panelTitle, panelText;
    private Button panelButton;
    private ProgressBar spinner;
    private View dot;
    private WebView web;
    private View fullscreen;
    private WebChromeClient.CustomViewCallback fullscreenCallback;
    private ValueCallback<Uri[]> pendingFiles;
    private boolean loaded, checking;
    private Prefs.Place pendingPlace;

    private final Runnable tick = new Runnable() {
        @Override public void run() { refresh(); main.postDelayed(this, loaded ? 5000 : 800); }
    };

    private int dp(float v) { return (int) TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, v, getResources().getDisplayMetrics()); }

    @Override protected void onCreate(Bundle state) {
        super.onCreate(state);
        edgeToEdge();
        buildLayout();
        if (Build.VERSION.SDK_INT >= 33 && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED)
            requestPermissions(new String[]{Manifest.permission.POST_NOTIFICATIONS}, ASK_NOTIFICATIONS);
        if (Prefs.wantsRunning(this)) ServerService.start(this);
    }

    @Override protected void onResume() { super.onResume(); main.removeCallbacks(tick); main.post(tick); }
    @Override protected void onPause() { super.onPause(); main.removeCallbacks(tick); CookieManager.getInstance().flush(); }

    /**
     * Android 15+ always draws apps under the status and navigation bars. Do it
     * on every version and pad by the real insets so the top bar never sits
     * under the clock/battery and the keyboard never covers inputs.
     */
    @SuppressWarnings("deprecation")
    private void edgeToEdge() {
        getWindow().setStatusBarColor(Color.TRANSPARENT);
        getWindow().setNavigationBarColor(Color.TRANSPARENT);
        if (Build.VERSION.SDK_INT >= 30) {
            getWindow().setDecorFitsSystemWindows(false);
        } else {
            getWindow().getDecorView().setSystemUiVisibility(View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                    | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                    | (Build.VERSION.SDK_INT >= 26 ? View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR : 0));
        }
        if (Build.VERSION.SDK_INT >= 29) getWindow().setNavigationBarContrastEnforced(false);
    }

    /** Light status icons over the dark bar, dark navigation icons over the light page. */
    private void barAppearance() {
        if (Build.VERSION.SDK_INT >= 30) {
            WindowInsetsController c = getWindow().getInsetsController();
            if (c != null) c.setSystemBarsAppearance(WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS,
                    WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS | WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS);
        }
    }

    @SuppressWarnings("deprecation")
    private WindowInsets applyInsets(View v, WindowInsets insets) {
        int top, bottom, left, right;
        if (Build.VERSION.SDK_INT >= 30) {
            android.graphics.Insets bars = insets.getInsets(WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout());
            android.graphics.Insets ime = insets.getInsets(WindowInsets.Type.ime());
            top = bars.top; left = bars.left; right = bars.right; bottom = Math.max(bars.bottom, ime.bottom);
        } else {
            top = insets.getSystemWindowInsetTop(); left = insets.getSystemWindowInsetLeft();
            right = insets.getSystemWindowInsetRight(); bottom = insets.getSystemWindowInsetBottom();
        }
        boolean full = fullscreen != null;
        // The dark bar extends behind the clock; content stops above the nav bar/keyboard.
        bar.setPadding(dp(16) + left, top, dp(4) + right, 0);
        bar.getLayoutParams().height = dp(52) + top;
        bar.requestLayout();
        content.setPadding(full ? 0 : left, 0, full ? 0 : right, full ? 0 : bottom);
        return insets;
    }

    private void buildLayout() {
        root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(PAPER);

        bar = new LinearLayout(this);
        bar.setOrientation(LinearLayout.HORIZONTAL);
        bar.setGravity(Gravity.CENTER_VERTICAL);
        bar.setBackgroundColor(DARK);
        bar.setPadding(dp(16), 0, dp(4), 0);
        TextView title = new TextView(this);
        title.setText("Sonam");
        title.setTextColor(Color.WHITE);
        title.setTextSize(17);
        title.setTypeface(Typeface.DEFAULT_BOLD);
        dot = new View(this);
        GradientDrawable round = new GradientDrawable();
        round.setShape(GradientDrawable.OVAL);
        round.setColor(Color.GRAY);
        dot.setBackground(round);
        LinearLayout.LayoutParams dotLp = new LinearLayout.LayoutParams(dp(8), dp(8));
        dotLp.setMargins(dp(14), 0, dp(7), 0);
        status = new TextView(this);
        status.setTextColor(Color.rgb(196, 207, 190));
        status.setTextSize(12);
        status.setSingleLine(true);
        status.setEllipsize(android.text.TextUtils.TruncateAt.END);
        status.setContentDescription("Endereço do servidor. Toque para ver e compartilhar");
        status.setOnClickListener(v -> addresses());
        ImageButton menu = new ImageButton(this);
        menu.setImageResource(R.drawable.ic_more);
        menu.setBackgroundColor(Color.TRANSPARENT);
        menu.setContentDescription("Opções do servidor");
        menu.setOnClickListener(this::showMenu);
        bar.addView(title);
        bar.addView(dot, dotLp);
        bar.addView(status, new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1));
        bar.addView(menu, new LinearLayout.LayoutParams(dp(48), dp(48)));
        root.addView(bar, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(52)));

        content = new FrameLayout(this);
        root.addView(content, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1));

        panel = new LinearLayout(this);
        panel.setOrientation(LinearLayout.VERTICAL);
        panel.setGravity(Gravity.CENTER);
        panel.setPadding(dp(32), dp(32), dp(32), dp(32));
        spinner = new ProgressBar(this);
        panelTitle = new TextView(this);
        panelTitle.setTextSize(22);
        panelTitle.setTextColor(Color.rgb(37, 51, 44));
        panelTitle.setGravity(Gravity.CENTER);
        panelTitle.setPadding(0, dp(18), 0, dp(8));
        panelText = new TextView(this);
        panelText.setTextSize(14);
        panelText.setTextColor(Color.rgb(110, 122, 104));
        panelText.setGravity(Gravity.CENTER);
        panelText.setLineSpacing(0, 1.3f);
        panelButton = new Button(this);
        panelButton.setAllCaps(false);
        panelButton.setTextColor(Color.WHITE);
        GradientDrawable btn = new GradientDrawable();
        btn.setColor(GREEN);
        btn.setCornerRadius(dp(8));
        panelButton.setBackground(btn);
        panelButton.setMinHeight(dp(48));
        panelButton.setPadding(dp(24), 0, dp(24), 0);
        LinearLayout.LayoutParams btnLp = new LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        btnLp.topMargin = dp(22);
        panel.addView(spinner);
        panel.addView(panelTitle);
        panel.addView(panelText);
        panel.addView(panelButton, btnLp);
        content.addView(panel, new FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
        setContentView(root);
        root.setOnApplyWindowInsetsListener(this::applyInsets);
        root.requestApplyInsets();
        barAppearance();
    }

    private void showPanel(boolean busy, String title, String text, String button, View.OnClickListener action) {
        panel.setVisibility(View.VISIBLE);
        if (web != null) web.setVisibility(View.GONE);
        spinner.setVisibility(busy ? View.VISIBLE : View.GONE);
        panelTitle.setText(title);
        panelText.setText(text);
        panelButton.setVisibility(button == null ? View.GONE : View.VISIBLE);
        panelButton.setText(button == null ? "" : button);
        panelButton.setOnClickListener(action);
    }

    private void setDot(int color) { ((GradientDrawable) dot.getBackground()).setColor(color); }

    /** Poll the service state; once the server answers, show the interface. */
    private void refresh() {
        String s = ServerService.state;
        String url = ServerService.bestUrl(this);
        String friendly = ServerService.nameUrl();
        if ("on".equals(s)) {
            setDot(Color.rgb(127, 199, 120));
            if (url == null) status.setText("Ligado · sem Wi-Fi");
            else status.setText((friendly != null ? friendly : url).replace("http://", ""));
            if (!loaded && !checking) probe();
        } else if ("error".equals(s)) {
            setDot(Color.rgb(220, 120, 100));
            status.setText("Com erro");
            loaded = false;
            showPanel(false, "O servidor não iniciou", ServerService.error == null ? "" : ServerService.error,
                    "Tentar de novo", v -> ServerService.start(this));
        } else if ("starting".equals(s) || ("off".equals(s) && Prefs.wantsRunning(this))) {
            setDot(Color.rgb(222, 190, 110));
            status.setText("Iniciando…");
            if (!loaded) showPanel(true, "Preparando sua casa…", "Ligando o servidor neste celular.", null, null);
        } else {
            setDot(Color.GRAY);
            status.setText("Desligado");
            loaded = false;
            showPanel(false, "Servidor desligado",
                    "Ligue para servir fotos, músicas, vídeos e a unidade de rede para os aparelhos da casa.",
                    "Ligar servidor", v -> ServerService.start(this));
        }
    }

    private void probe() {
        checking = true;
        new Thread(() -> {
            boolean ok = false;
            try {
                HttpURLConnection c = (HttpURLConnection) new URL(ORIGIN + "/api/status").openConnection();
                c.setConnectTimeout(1500);
                c.setReadTimeout(1500);
                ok = c.getResponseCode() == 200;
                c.disconnect();
            } catch (Exception ignored) {}
            final boolean ready = ok;
            main.post(() -> { checking = false; if (ready && !loaded) showWeb(); });
        }, "sonam-probe").start();
    }

    private void showWeb() {
        loaded = true;
        panel.setVisibility(View.GONE);
        if (web == null) {
            web = new WebView(this);
            WebSettings s = web.getSettings();
            s.setJavaScriptEnabled(true);
            s.setDomStorageEnabled(true);
            s.setAllowFileAccess(false);
            s.setAllowContentAccess(false);
            s.setMixedContentMode(WebSettings.MIXED_CONTENT_NEVER_ALLOW);
            s.setMediaPlaybackRequiresUserGesture(false);
            CookieManager.getInstance().setAcceptCookie(true);
            CookieManager.getInstance().setAcceptThirdPartyCookies(web, false);
            web.setWebViewClient(new WebViewClient() {
                @Override public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest req) {
                    Uri u = req.getUrl();
                    if ((u.getScheme() + "://" + u.getEncodedAuthority()).equals(ORIGIN)) return false;
                    if ("http".equals(u.getScheme()) || "https".equals(u.getScheme())) {
                        try { startActivity(new Intent(Intent.ACTION_VIEW, u)); } catch (ActivityNotFoundException ignored) {}
                    }
                    return true;
                }
            });
            web.setWebChromeClient(new WebChromeClient() {
                @Override public boolean onShowFileChooser(WebView view, ValueCallback<Uri[]> callback, FileChooserParams params) {
                    if (pendingFiles != null) pendingFiles.onReceiveValue(null);
                    pendingFiles = callback;
                    Intent pick = new Intent(Intent.ACTION_GET_CONTENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*");
                    pick.putExtra(Intent.EXTRA_MIME_TYPES, new String[]{"image/*", "video/*", "audio/*"});
                    pick.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, params.getMode() == FileChooserParams.MODE_OPEN_MULTIPLE);
                    try { startActivityForResult(Intent.createChooser(pick, "Escolher arquivos"), PICK_FILES); }
                    catch (ActivityNotFoundException e) { pendingFiles = null; return false; }
                    return true;
                }
                @Override public void onShowCustomView(View view, CustomViewCallback callback) {
                    if (fullscreen != null) { callback.onCustomViewHidden(); return; }
                    fullscreen = view;
                    fullscreenCallback = callback;
                    bar.setVisibility(View.GONE);
                    content.addView(view, new FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
                    systemBars(false);
                }
                @Override public void onHideCustomView() { exitFullscreen(); }
            });
            web.setDownloadListener((url, agent, disposition, mime, length) ->
                    Toast.makeText(this, "Este download é para o computador. Abra o Sonam nele.", Toast.LENGTH_LONG).show());
            content.addView(web, 0, new FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
            web.loadUrl(ORIGIN + "/");
        } else {
            web.reload();
        }
        web.setVisibility(View.VISIBLE);
    }

    private void exitFullscreen() {
        if (fullscreen == null) return;
        content.removeView(fullscreen);
        fullscreen = null;
        bar.setVisibility(View.VISIBLE);
        systemBars(true);
        if (fullscreenCallback != null) { fullscreenCallback.onCustomViewHidden(); fullscreenCallback = null; }
    }

    @SuppressWarnings("deprecation")
    private void systemBars(boolean visible) {
        if (Build.VERSION.SDK_INT >= 30) {
            WindowInsetsController c = getWindow().getInsetsController();
            if (c == null) return;
            if (visible) c.show(WindowInsets.Type.systemBars());
            else {
                c.hide(WindowInsets.Type.systemBars());
                c.setSystemBarsBehavior(WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
            }
        } else if (visible) {
            getWindow().clearFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN);
        } else {
            getWindow().addFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN);
        }
        root.requestApplyInsets();
    }

    @Override protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request == PICK_FILES && pendingFiles != null) {
            Uri[] picked = null;
            if (result == RESULT_OK && data != null) {
                if (data.getClipData() != null) {
                    picked = new Uri[data.getClipData().getItemCount()];
                    for (int i = 0; i < picked.length; i++) picked[i] = data.getClipData().getItemAt(i).getUri();
                } else if (data.getData() != null) picked = new Uri[]{data.getData()};
            }
            pendingFiles.onReceiveValue(picked);
            pendingFiles = null;
        }
    }

    @Override public void onBackPressed() {
        if (fullscreen != null) { exitFullscreen(); return; }
        if (web == null || !loaded) { moveTaskToBack(true); return; }
        // Close an open dialog/player first; otherwise just leave (server keeps running).
        web.evaluateJavascript("(function(){if(document.querySelector('.modal-backdrop,.viewer')){"
                + "window.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));return 1}return 0})()",
                r -> { if (!"1".equals(r)) moveTaskToBack(true); });
    }

    // ----- Server options -------------------------------------------------

    private void showMenu(View anchor) {
        PopupMenu m = new PopupMenu(this, anchor);
        boolean on = "on".equals(ServerService.state) || "starting".equals(ServerService.state);
        m.getMenu().add(0, 1, 0, "Endereço e compartilhar");
        m.getMenu().add(0, 2, 1, "Iniciar junto com o celular").setCheckable(true).setChecked(Prefs.startOnBoot(this));
        m.getMenu().add(0, 3, 2, "Funcionar com a tela apagada");
        m.getMenu().add(0, 4, 3, "Onde guardar os arquivos");
        m.getMenu().add(0, 5, 4, "Dicas para usar como servidor");
        m.getMenu().add(0, 6, 5, on ? "Desligar servidor" : "Ligar servidor");
        m.setOnMenuItemClickListener(item -> {
            switch (item.getItemId()) {
                case 1: addresses(); return true;
                case 2: Prefs.setStartOnBoot(this, !item.isChecked()); return true;
                case 3: battery(); return true;
                case 4: chooseStorage(); return true;
                case 5: tips(); return true;
                case 6:
                    if (on) { ServerService.stop(this); loaded = false; }
                    else ServerService.start(this);
                    refresh();
                    return true;
                default: return false;
            }
        });
        m.show();
    }

    /** Both addresses, explained, with copy and share. */
    private void addresses() {
        String ip = ServerService.bestUrl(this), friendly = ServerService.nameUrl();
        if (ip == null) { Toast.makeText(this, "Conecte o celular ao Wi-Fi de casa primeiro.", Toast.LENGTH_LONG).show(); return; }
        StringBuilder msg = new StringBuilder();
        if (friendly != null) msg.append("Pelo nome (não muda quando o IP muda):\n").append(friendly)
                .append("\nFunciona em computadores, iPhone e na maioria dos aparelhos. O nome pode ser trocado em Configurações › Nome na rede.\n\n");
        msg.append("Pelo IP (funciona em qualquer aparelho, inclusive TVs):\n").append(ip)
                .append("\n\nNa TV, adicione /?tv=1 ao endereço.");
        final String copy = friendly != null ? friendly : ip;
        new AlertDialog.Builder(this).setTitle("Endereço da sua casa").setMessage(msg)
                .setPositiveButton("Compartilhar", (d, w) -> share())
                .setNeutralButton("Copiar", (d, w) -> {
                    android.content.ClipboardManager cb = (android.content.ClipboardManager) getSystemService(CLIPBOARD_SERVICE);
                    cb.setPrimaryClip(android.content.ClipData.newPlainText("Sonam", copy));
                    Toast.makeText(this, "Endereço copiado", Toast.LENGTH_SHORT).show();
                })
                .setNegativeButton("Fechar", null).show();
    }

    private void share() {
        String url = ServerService.bestUrl(this), friendly = ServerService.nameUrl();
        if (url == null) { Toast.makeText(this, "Conecte o celular ao Wi-Fi de casa primeiro.", Toast.LENGTH_LONG).show(); return; }
        String text = "Abra o Sonam da nossa casa: " + (friendly != null ? friendly + "\nSe não abrir, use " : "") + url
                + "\nNa TV, use " + url + "/?tv=1";
        Intent send = new Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, text);
        startActivity(Intent.createChooser(send, "Compartilhar endereço"));
    }

    private void battery() {
        PowerManager pm = (PowerManager) getSystemService(POWER_SERVICE);
        try {
            if (Build.VERSION.SDK_INT >= 23 && !pm.isIgnoringBatteryOptimizations(getPackageName())) {
                startActivity(new Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:" + getPackageName())));
            } else {
                new AlertDialog.Builder(this).setTitle("Tudo certo")
                        .setMessage("O Android já deixa o Sonam funcionar com a tela apagada. Alguns fabricantes (Xiaomi, Samsung, Huawei) têm uma economia de bateria própria: em Configurações › Bateria, marque o Sonam como \"sem restrições\".")
                        .setPositiveButton("Ok", null).setNeutralButton("Abrir configurações", (d, w) ->
                                startActivity(new Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:" + getPackageName()))))
                        .show();
            }
        } catch (ActivityNotFoundException e) {
            startActivity(new Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:" + getPackageName())));
        }
    }

    private void tips() {
        new AlertDialog.Builder(this).setTitle("Celular antigo como servidor")
                .setMessage("• Deixe o celular na tomada. Se o aparelho tiver \"proteção de bateria\" ou limite de carga em 80–85%, ative: ele fica ligado anos sem estufar a bateria.\n\n"
                        + "• Toque em \"Funcionar com a tela apagada\" e libere o app da economia de bateria.\n\n"
                        + "• No roteador, reserve um IP fixo (reserva DHCP) para este celular. O nome .local também ajuda quando o IP muda.\n\n"
                        + "• Para arquivos grandes, use um cartão SD em \"Onde guardar os arquivos\".\n\n"
                        + "• No computador, abra Configurações › Unidade de rede para usar o celular como um disco.\n\n"
                        + "• Fora de casa: instale o Tailscale neste celular e nos seus aparelhos.")
                .setPositiveButton("Entendi", null).show();
    }

    private boolean hasPublicAccess() {
        if (Build.VERSION.SDK_INT >= 30) return Prefs.hasAllFilesAccess();
        return checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE) == PackageManager.PERMISSION_GRANTED;
    }

    private void chooseStorage() {
        final List<Prefs.Place> places = Prefs.places(this);
        String current = Prefs.dataDir(this).getAbsolutePath();
        String[] labels = new String[places.size()];
        int checked = 0;
        for (int i = 0; i < places.size(); i++) {
            Prefs.Place p = places.get(i);
            labels[i] = p.label + " · " + Formatter.formatShortFileSize(this, p.free()) + " livres";
            if (p.dir.getAbsolutePath().equals(current)) checked = i;
        }
        new AlertDialog.Builder(this).setTitle("Onde guardar os arquivos")
                .setSingleChoiceItems(labels, checked, (d, which) -> {
                    d.dismiss();
                    Prefs.Place p = places.get(which);
                    if (p.dir.getAbsolutePath().equals(current)) return;
                    confirmPlace(p);
                })
                .setNegativeButton("Cancelar", null).show();
    }

    private void confirmPlace(Prefs.Place p) {
        new AlertDialog.Builder(this).setTitle("Trocar o local?")
                .setMessage("O servidor vai reiniciar usando \"" + p.label + "\". A biblioteca atual continua no local antigo, e você pode voltar a ela quando quiser."
                        + (p.needsPermission ? "\n\nEsta pasta fica em Armazenamento interno › Sonam e precisa da permissão de acesso a arquivos." : "\n\nPastas do app são apagadas se o app for desinstalado (o Android pergunta antes)."))
                .setPositiveButton("Trocar", (d, w) -> {
                    if (p.needsPermission && !hasPublicAccess()) { pendingPlace = p; askStorage(); return; }
                    applyPlace(p);
                })
                .setNegativeButton("Cancelar", null).show();
    }

    private void askStorage() {
        if (Build.VERSION.SDK_INT >= 30) {
            try {
                startActivityForResult(new Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION, Uri.parse("package:" + getPackageName())), ASK_STORAGE);
            } catch (ActivityNotFoundException e) {
                startActivityForResult(new Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION), ASK_STORAGE);
            }
        } else {
            requestPermissions(new String[]{Manifest.permission.WRITE_EXTERNAL_STORAGE}, ASK_STORAGE);
        }
    }

    @Override public void onRequestPermissionsResult(int request, String[] permissions, int[] results) {
        super.onRequestPermissionsResult(request, permissions, results);
        if (request == ASK_STORAGE) afterStoragePermission();
    }

    private void afterStoragePermission() {
        if (pendingPlace == null) return;
        Prefs.Place p = pendingPlace;
        pendingPlace = null;
        if (hasPublicAccess()) applyPlace(p);
        else Toast.makeText(this, "Sem permissão, o Sonam continua no local atual.", Toast.LENGTH_LONG).show();
    }

    @Override protected void onRestart() {
        super.onRestart();
        if (pendingPlace != null && Build.VERSION.SDK_INT >= 30) afterStoragePermission();
    }

    private void applyPlace(Prefs.Place p) {
        File dir = p.dir;
        if (!dir.exists() && !dir.mkdirs()) {
            Toast.makeText(this, "Não foi possível usar " + dir, Toast.LENGTH_LONG).show();
            return;
        }
        Prefs.setDataDir(this, dir);
        loaded = false;
        showPanel(true, "Reiniciando…", "Abrindo a biblioteca em " + p.label + ".", null, null);
        ServerService.stop(this);
        main.postDelayed(() -> ServerService.start(this), 1500);
    }

    @Override protected void onDestroy() {
        if (web != null) { web.destroy(); web = null; }
        super.onDestroy();
    }
}
