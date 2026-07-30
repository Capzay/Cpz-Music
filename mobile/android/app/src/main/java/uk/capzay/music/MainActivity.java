package uk.capzay.music;

import android.Manifest;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.webkit.CookieManager;
import android.webkit.WebResourceRequest;
import android.webkit.WebView;

import androidx.core.app.ActivityCompat;
import androidx.core.content.ContextCompat;

import com.getcapacitor.BridgeActivity;
import com.getcapacitor.BridgeWebViewClient;

public class MainActivity extends BridgeActivity {

    private static final int NOTIFICATION_PERMISSION_REQUEST = 1001;

    @Override
    public void onCreate(Bundle savedInstanceState) {
        registerPlugin(MediaSessionPlugin.class);
        super.onCreate(savedInstanceState);
        requestNotificationPermission();
        startPlaybackService();
        setupWebView();
    }

    /**
     * Keeps the WebView's JavaScript running while the screen is locked or the
     * app is backgrounded. Without this Android suspends the WebView, audio
     * stops, and the lock-screen controls disappear mid-track.
     */
    @Override
    public void onPause() {
        super.onPause();
        resumeWebView();
    }

    @Override
    public void onStop() {
        super.onStop();
        resumeWebView();
    }

    private void resumeWebView() {
        if (getBridge() != null && getBridge().getWebView() != null) {
            getBridge().getWebView().onResume();
        }
    }

    private void setupWebView() {
        CookieManager cookies = CookieManager.getInstance();
        cookies.setAcceptCookie(true);
        cookies.setAcceptThirdPartyCookies(getBridge().getWebView(), true);

        // Taken from capacitor.config.ts rather than hardcoded, so pointing the
        // app at a different deployment does not mean editing Java.
        final String serverHost = serverHost();

        getBridge().getWebView().setWebViewClient(new BridgeWebViewClient(getBridge()) {
            @Override
            public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
                Uri uri = request.getUrl();
                if (serverHost != null && serverHost.equals(uri.getHost())) return false;
                // Anything else belongs to somebody else. Hand it to the browser
                // rather than navigating this authenticated WebView there.
                startActivity(new Intent(Intent.ACTION_VIEW, uri));
                return true;
            }
        });
    }

    private String serverHost() {
        try {
            String url = getBridge().getConfig().getServerUrl();
            return url == null ? null : Uri.parse(url).getHost();
        } catch (Exception e) {
            return null;
        }
    }

    private void requestNotificationPermission() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU
                && ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS)
                    != PackageManager.PERMISSION_GRANTED) {
            ActivityCompat.requestPermissions(
                this,
                new String[]{Manifest.permission.POST_NOTIFICATIONS},
                NOTIFICATION_PERMISSION_REQUEST);
        }
    }

    private void startPlaybackService() {
        Intent intent = new Intent(this, MediaPlaybackService.class);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            startForegroundService(intent);
        } else {
            startService(intent);
        }
    }
}
