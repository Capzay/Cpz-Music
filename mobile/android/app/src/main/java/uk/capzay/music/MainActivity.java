package uk.capzay.music;

import android.Manifest;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.os.Build;
import android.os.Bundle;
import android.webkit.CookieManager;

import androidx.core.app.ActivityCompat;
import androidx.core.content.ContextCompat;

import com.getcapacitor.BridgeActivity;

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

    /**
     * Capacitor's own client already sends anything outside the server host and
     * `server.allowNavigation` to the system browser, so overriding it here only
     * dropped the allowlist on the floor and pushed the sign-in hop out of the
     * app. Cookies still need turning on by hand.
     */
    private void setupWebView() {
        CookieManager cookies = CookieManager.getInstance();
        cookies.setAcceptCookie(true);
        cookies.setAcceptThirdPartyCookies(getBridge().getWebView(), true);
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
