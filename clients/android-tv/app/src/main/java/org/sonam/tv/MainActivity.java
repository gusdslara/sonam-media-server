package org.sonam.tv;

import android.app.Activity;
import android.app.AlertDialog;
import android.graphics.Color;
import android.net.Uri;
import android.os.Bundle;
import android.view.KeyEvent;
import android.view.View;
import android.webkit.CookieManager;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceRequest;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.TextView;

/** A small TV launcher. Playback and pairing use the same audited server API as the web app. */
public final class MainActivity extends Activity {
    private WebView web;
    private String origin;
    private View fullscreen;
    private WebChromeClient.CustomViewCallback fullscreenCallback;

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        String saved = getPreferences(MODE_PRIVATE).getString("server", "");
        if (saved.isEmpty()) configure(); else connect(saved);
    }

    private void configure() {
        if (web != null) { web.destroy(); web = null; }
        LinearLayout layout = new LinearLayout(this);
        layout.setOrientation(LinearLayout.VERTICAL);
        layout.setPadding(70, 45, 70, 45);
        layout.setBackgroundColor(Color.rgb(23, 37, 30));
        TextView title = new TextView(this);
        title.setText("Sonam · Sua casa na TV"); title.setTextColor(Color.WHITE); title.setTextSize(29);
        TextView help = new TextView(this);
        help.setText("Digite o IP que aparece no Sonam do seu PC ou celular.\nExemplo: http://192.168.1.10:8096\nDepois, escolha entrar com código para parear. Segure Voltar para trocar o servidor.");
        help.setTextColor(Color.LTGRAY); help.setTextSize(18); help.setPadding(0, 20, 0, 20);
        EditText address = new EditText(this); address.setSingleLine(true); address.setTextColor(Color.WHITE);
        address.setText(getPreferences(MODE_PRIVATE).getString("server", "http://")); address.setInputType(17);
        Button connect = new Button(this); connect.setText("Conectar à minha casa");
        connect.setOnClickListener(v -> { try { connect(address.getText().toString().trim()); } catch (IllegalArgumentException e) { address.setError(e.getMessage()); } });
        layout.addView(title); layout.addView(help); layout.addView(address); layout.addView(connect);
        setContentView(layout); address.requestFocus();
    }

    private void connect(String value) {
        Uri uri = Uri.parse(value);
        String host = uri.getHost();
        if (host == null || !("http".equals(uri.getScheme()) || "https".equals(uri.getScheme())) || uri.getUserInfo() != null || !localHost(host))
            throw new IllegalArgumentException("Use um IP da rede local, sem usuário ou senha na URL.");
        origin = uri.getScheme() + "://" + uri.getEncodedAuthority();
        getPreferences(MODE_PRIVATE).edit().putString("server", origin).apply();
        web = new WebView(this);
        WebSettings s = web.getSettings();
        s.setJavaScriptEnabled(true); s.setDomStorageEnabled(true); s.setAllowFileAccess(false); s.setAllowContentAccess(false);
        s.setMixedContentMode(WebSettings.MIXED_CONTENT_NEVER_ALLOW); s.setMediaPlaybackRequiresUserGesture(false);
        CookieManager.getInstance().setAcceptCookie(true); CookieManager.getInstance().setAcceptThirdPartyCookies(web, false);
        web.setWebViewClient(new WebViewClient() {
            @Override public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest req) {
                Uri u = req.getUrl();
                return !(u.getScheme() + "://" + u.getEncodedAuthority()).equals(origin);
            }
        });
        web.setWebChromeClient(new WebChromeClient() {
            @Override public void onShowCustomView(View view, CustomViewCallback callback) {
                if (fullscreen != null) { callback.onCustomViewHidden(); return; }
                fullscreen = view; fullscreenCallback = callback; setContentView(view);
            }
            @Override public void onHideCustomView() { exitFullscreen(); }
        });
        setContentView(web); web.loadUrl(origin + "/?tv=1"); web.requestFocus();
    }

    private boolean localHost(String host) {
        if ("localhost".equals(host) || "127.0.0.1".equals(host)) return true;
        String[] parts = host.split("\\."); if (parts.length != 4) return false;
        int[] p = new int[4];
        try { for (int i=0; i<4; i++) { p[i] = Integer.parseInt(parts[i]); if (p[i]<0 || p[i]>255) return false; } } catch (NumberFormatException e) { return false; }
        return p[0]==10 || p[0]==192 && p[1]==168 || p[0]==172 && p[1]>=16 && p[1]<=31;
    }

    private void exitFullscreen() {
        if (fullscreen == null) return;
        fullscreen = null; setContentView(web);
        if (fullscreenCallback != null) { fullscreenCallback.onCustomViewHidden(); fullscreenCallback = null; }
    }
    @Override public boolean onKeyDown(int key, KeyEvent event) {
        if (key == KeyEvent.KEYCODE_BACK && event.getRepeatCount() == 0) {
            event.startTracking();
            return true;
        }
        return super.onKeyDown(key, event);
    }
    @Override public boolean onKeyLongPress(int key, KeyEvent event) {
        if (key == KeyEvent.KEYCODE_BACK) { configure(); return true; }
        return super.onKeyLongPress(key, event);
    }
    @Override public void onBackPressed() {
        if (fullscreen != null) exitFullscreen();
        else if (web != null) { web.evaluateJavascript("window.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}))", null); }
        else super.onBackPressed();
    }
    @Override protected void onPause() { super.onPause(); CookieManager.getInstance().flush(); }
    @Override protected void onDestroy() { if (web != null) web.destroy(); super.onDestroy(); }
}
