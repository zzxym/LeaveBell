package com.leavebell.screen;

import android.os.Bundle;
import android.os.PowerManager;
import android.provider.Settings;
import android.content.Intent;
import android.content.SharedPreferences;
import android.net.Uri;
import android.view.KeyEvent;
import android.view.View;
import android.view.MotionEvent;
import android.widget.EditText;
import android.widget.Toast;
import android.app.AlertDialog;
import android.webkit.WebChromeClient;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.app.Activity;

public class MainActivity extends Activity {
    private static final String PREF_NAME = "leavebell_screen";
    private static final String PREF_URL = "controller_url";

    private WebView web;
    private PowerManager.WakeLock wakeLock;
    private SharedPreferences prefs;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(R.layout.activity_main);
        web = findViewById(R.id.webview);

        WebSettings s = web.getSettings();
        s.setJavaScriptEnabled(true);
        s.setMediaPlaybackRequiresUserGesture(false); // 允许自动语音播报
        s.setDomStorageEnabled(true);
        // 不启用 WebView 的 overview/wideViewport 缩放：页面内 fitStage() 已按屏幕等比缩放画布，
        // 两者叠加会导致班级名被双重放大溢出
        s.setTextZoom(100);
        s.setUserAgentString(s.getUserAgentString() + " LeaveBellScreen/1.0");
        s.setCacheMode(WebSettings.LOAD_NO_CACHE);
        web.setWebChromeClient(new WebChromeClient());
        web.setWebViewClient(new WebViewClient());
        web.setBackgroundResource(android.R.color.black);

        prefs = getSharedPreferences(PREF_NAME, MODE_PRIVATE);
        String url = prefs.getString(PREF_URL, "");
        if (url.isEmpty()) {
            showUrlDialog(); // 首次启动：让用户填 IP 和端口
        } else {
            web.loadUrl(url);
        }
        web.setOnLongClickListener(v -> { showUrlDialog(); return true; });

        // 全屏沉浸
        getWindow().getDecorView().setSystemUiVisibility(
            View.SYSTEM_UI_FLAG_FULLSCREEN
            | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
            | View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
            | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
            | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
            | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION);

        // 持有部分 WakeLock：屏幕可灭但 CPU 不休眠，保证轮询/SSE 与播报不被冻结
        PowerManager pm = (PowerManager) getSystemService(POWER_SERVICE);
        wakeLock = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "LeaveBell:Screen");
        wakeLock.acquire();

        // 引导加入电池白名单：未忽略优化时直接打开系统设置页让用户授权
        if (!pm.isIgnoringBatteryOptimizations(getPackageName())) {
            try {
                Intent intent = new Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS);
                intent.setData(Uri.parse("package:" + getPackageName()));
                startActivity(intent);
            } catch (Exception ignored) {
                // 部分设备不支持该设置页，跳过
            }
        }
    }

    private void showUrlDialog() {
        EditText input = new EditText(this);
        input.setHint("http://192.168.1.100:80");
        input.setText(prefs.getString(PREF_URL, ""));
        new AlertDialog.Builder(this)
            .setTitle("控制器地址")
            .setMessage("输入 IP 和端口，例如 http://192.168.1.100:80")
            .setView(input)
            .setCancelable(false)
            .setPositiveButton("连接", (d, w) -> {
                String url = input.getText().toString().trim();
                if (!url.startsWith("http://") && !url.startsWith("https://")) url = "http://" + url;
                prefs.edit().putString(PREF_URL, url).apply();
                web.loadUrl(url);
                Toast.makeText(this, "已设置：" + url, Toast.LENGTH_SHORT).show();
            })
            .setNegativeButton("取消", null)
            .show();
    }

    @Override
    public boolean onKeyDown(int keyCode, KeyEvent event) {
        if (keyCode == KeyEvent.KEYCODE_BACK) return true; // kiosk：不退出
        return super.onKeyDown(keyCode, event);
    }

    @Override
    protected void onResume() {
        super.onResume();
        if (web != null) web.resumeTimers();
    }

    @Override
    protected void onPause() {
        if (web != null) web.pauseTimers();
        super.onPause();
    }

    @Override
    protected void onDestroy() {
        if (wakeLock != null && wakeLock.isHeld()) wakeLock.release();
        super.onDestroy();
    }
}
