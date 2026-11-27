package uk.capzay.music;

import android.Manifest;
import android.app.NativeActivity;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.provider.Settings;
import android.util.Log;

import java.io.File;

/**
 * The Rust app (crates/app, built as libcpz_app.so) runs inside this activity
 * and draws with Slint. Java only does what needs the Android framework:
 * deep links, the playback notification, the device name and installing updates.
 * The public methods are called from Rust (src/android.rs), on any thread.
 */
public class MainActivity extends NativeActivity {
    static final String TAG = "cpz-music";

    static {
        // Loaded here as well as by NativeActivity so the native methods below
        // resolve against this class's loader.
        System.loadLibrary("cpz_app");
    }

    /** "play", "pause", "toggle", "next" or "prev". */
    static native void nativeCommand(String command);

    /** The uk.capzay.music://auth/callback?code=… the browser came back with. */
    static native void nativeDeepLink(String url);

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        deliver(getIntent());
        if (Build.VERSION.SDK_INT >= 33
                && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)
                        != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(new String[] {Manifest.permission.POST_NOTIFICATIONS}, 1);
        }
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        deliver(intent);
    }

    private void deliver(Intent intent) {
        Uri data = intent == null ? null : intent.getData();
        if (data != null && "uk.capzay.music".equals(data.getScheme())) {
            nativeDeepLink(data.toString());
        }
    }

    public void nowPlaying(String title, String artist, boolean playing) {
        PlaybackService.update(getApplicationContext(), title, artist, playing);
    }

    public void moveToBack() {
        runOnUiThread(() -> moveTaskToBack(true));
    }

    public String deviceName() {
        String name = Settings.Global.getString(getContentResolver(), Settings.Global.DEVICE_NAME);
        return name != null && !name.isEmpty() ? name : Build.MODEL;
    }

    public boolean installApk(String path) {
        try {
            InstallReceiver.install(this, new File(path));
            return true;
        } catch (Exception e) {
            Log.w(TAG, "could not start the install", e);
            return false;
        }
    }
}
