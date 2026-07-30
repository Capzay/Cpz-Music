package uk.capzay.music;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.Context;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.os.SystemClock;
import android.support.v4.media.MediaMetadataCompat;
import android.support.v4.media.session.MediaSessionCompat;
import android.support.v4.media.session.PlaybackStateCompat;
import android.util.Log;
import android.webkit.CookieManager;

import androidx.core.app.NotificationCompat;
import androidx.media.app.NotificationCompat.MediaStyle;

import com.getcapacitor.Plugin;
import com.getcapacitor.PluginCall;
import com.getcapacitor.PluginMethod;
import com.getcapacitor.annotation.CapacitorPlugin;

import java.io.InputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.util.Locale;

/**
 * Native lock-screen controls.
 *
 * Chrome's own MediaSession is not enough inside a WebView: Android suspends the
 * WebView when the screen locks and the session goes with it. A real
 * MediaSessionCompat, owned by a foreground service, survives that.
 */
@CapacitorPlugin(name = "NativeMediaSession")
public class MediaSessionPlugin extends Plugin {

    private static final String TAG = "CpzMediaSession";
    private static final String CHANNEL_ID = "cpz_media_session";
    /** Notification art is displayed small; anything larger is wasted memory. */
    private static final int MAX_ARTWORK_PX = 512;

    static MediaSessionPlugin instance;

    private MediaSessionCompat mediaSession;
    private String currentTitle = "";
    private String currentArtist = "";
    private String currentAlbum = "";
    private boolean isPlaying = false;
    private long positionMs = 0;
    private long durationMs = 0;
    private Bitmap artwork = null;
    private String lastArtworkUrl = "";

    @Override
    public void load() {
        instance = this;
        createNotificationChannel();
        setupMediaSession();
    }

    private void setupMediaSession() {
        mediaSession = new MediaSessionCompat(getContext(), "CpzMusic");
        mediaSession.setFlags(
            MediaSessionCompat.FLAG_HANDLES_MEDIA_BUTTONS
                | MediaSessionCompat.FLAG_HANDLES_TRANSPORT_CONTROLS);
        mediaSession.setCallback(new MediaSessionCompat.Callback() {
            @Override public void onPlay() { fireAction("play", null); }
            @Override public void onPause() { fireAction("pause", null); }
            @Override public void onSkipToNext() { fireAction("next", null); }
            @Override public void onSkipToPrevious() { fireAction("previous", null); }
            @Override public void onSeekTo(long posMs) { fireAction("seek", posMs / 1000.0); }
        });
        mediaSession.setActive(true);
    }

    static void handleAction(String action) {
        if (instance == null) return;
        String resolved = "play_pause".equals(action)
            ? (instance.isPlaying ? "pause" : "play")
            : action;
        instance.fireAction(resolved, null);
    }

    /**
     * Delivers a media action to the page through evaluateJavascript, which runs
     * even while the activity is stopped. Capacitor's own event bridge does not,
     * which is exactly when lock-screen buttons get pressed.
     */
    private void fireAction(String action, Double seekTime) {
        // Whitelisted rather than interpolated blindly: this string becomes
        // JavaScript, and the set of valid actions is closed.
        switch (action) {
            case "play":
            case "pause":
            case "next":
            case "previous":
            case "seek":
                break;
            default:
                return;
        }

        String js = seekTime != null
            ? String.format(Locale.ROOT,
                "window.__cpzMediaAction && window.__cpzMediaAction('%s',%f);", action, seekTime)
            : String.format(Locale.ROOT,
                "window.__cpzMediaAction && window.__cpzMediaAction('%s');", action);

        if (getBridge() != null && getBridge().getWebView() != null) {
            getBridge().getWebView().post(
                () -> getBridge().getWebView().evaluateJavascript(js, null));
        }
    }

    @PluginMethod
    public void update(PluginCall call) {
        currentTitle = call.getString("title", "");
        currentArtist = call.getString("artist", "");
        currentAlbum = call.getString("album", "");
        isPlaying = Boolean.TRUE.equals(call.getBoolean("playing", false));
        positionMs = (long) (call.getDouble("position", 0.0) * 1000);
        durationMs = (long) (call.getDouble("duration", 0.0) * 1000);

        mediaSession.setMetadata(new MediaMetadataCompat.Builder()
            .putString(MediaMetadataCompat.METADATA_KEY_TITLE, currentTitle)
            .putString(MediaMetadataCompat.METADATA_KEY_ARTIST, currentArtist)
            .putString(MediaMetadataCompat.METADATA_KEY_ALBUM, currentAlbum)
            .putLong(MediaMetadataCompat.METADATA_KEY_DURATION, durationMs)
            .build());

        pushPlaybackState();

        String artworkUrl = call.getString("artworkUrl", "");
        if (artworkUrl != null && !artworkUrl.isEmpty() && !artworkUrl.equals(lastArtworkUrl)) {
            lastArtworkUrl = artworkUrl;
            fetchArtwork(artworkUrl);
        } else {
            if (artworkUrl == null || artworkUrl.isEmpty()) {
                artwork = null;
                lastArtworkUrl = "";
            }
            pushNotification();
        }

        call.resolve();
    }

    Notification buildCurrentNotification() {
        return buildNotification();
    }

    private void pushPlaybackState() {
        long actions = PlaybackStateCompat.ACTION_PLAY_PAUSE
            | PlaybackStateCompat.ACTION_PLAY
            | PlaybackStateCompat.ACTION_PAUSE
            | PlaybackStateCompat.ACTION_SKIP_TO_NEXT
            | PlaybackStateCompat.ACTION_SKIP_TO_PREVIOUS
            | PlaybackStateCompat.ACTION_SEEK_TO;

        mediaSession.setPlaybackState(new PlaybackStateCompat.Builder()
            .setState(
                isPlaying ? PlaybackStateCompat.STATE_PLAYING : PlaybackStateCompat.STATE_PAUSED,
                positionMs,
                1.0f,
                SystemClock.elapsedRealtime())
            .setActions(actions)
            .build());
    }

    private void fetchArtwork(String urlStr) {
        new Thread(() -> {
            Bitmap bitmap = null;
            try {
                HttpURLConnection connection = (HttpURLConnection) new URL(urlStr).openConnection();
                connection.setInstanceFollowRedirects(true);
                connection.setConnectTimeout(8000);
                connection.setReadTimeout(15000);
                connection.setRequestProperty("User-Agent", "CpzMusicAndroid");

                // Artwork sits behind the same auth as everything else, so this
                // request has to carry the WebView's session cookie. Without it
                // the server answers 401 and the lock screen shows no art.
                String cookies = CookieManager.getInstance().getCookie(urlStr);
                if (cookies != null && !cookies.isEmpty()) {
                    connection.setRequestProperty("Cookie", cookies);
                }

                connection.connect();
                int code = connection.getResponseCode();
                if (code == HttpURLConnection.HTTP_OK) {
                    try (InputStream stream = connection.getInputStream()) {
                        bitmap = decodeScaled(stream);
                    }
                } else {
                    Log.w(TAG, "artwork fetch returned HTTP " + code);
                }
            } catch (Exception e) {
                Log.w(TAG, "artwork fetch failed: " + e.getMessage());
            }

            final Bitmap result = bitmap;
            new Handler(Looper.getMainLooper()).post(() -> {
                artwork = result;
                pushNotification();
            });
        }).start();
    }

    /** Caps the decoded size, so a large cover cannot push the app out of memory. */
    private Bitmap decodeScaled(InputStream stream) {
        BitmapFactory.Options options = new BitmapFactory.Options();
        options.inJustDecodeBounds = false;
        Bitmap decoded = BitmapFactory.decodeStream(stream, null, options);
        if (decoded == null) return null;

        int largest = Math.max(decoded.getWidth(), decoded.getHeight());
        if (largest <= MAX_ARTWORK_PX) return decoded;

        float scale = (float) MAX_ARTWORK_PX / largest;
        Bitmap scaled = Bitmap.createScaledBitmap(
            decoded,
            Math.round(decoded.getWidth() * scale),
            Math.round(decoded.getHeight() * scale),
            true);
        if (scaled != decoded) decoded.recycle();
        return scaled;
    }

    private void pushNotification() {
        if (mediaSession == null) return;
        Notification notification = buildNotification();

        if (MediaPlaybackService.instance != null) {
            MediaPlaybackService.instance.promoteToMedia(notification);
        } else {
            NotificationManager manager =
                (NotificationManager) getContext().getSystemService(Context.NOTIFICATION_SERVICE);
            if (manager != null) manager.notify(MediaPlaybackService.NOTIF_ID, notification);
        }
    }

    private Notification buildNotification() {
        Intent launch = new Intent(getContext(), MainActivity.class);
        launch.setFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP);
        PendingIntent contentIntent = PendingIntent.getActivity(
            getContext(), 0, launch,
            PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);

        NotificationCompat.Builder builder =
            new NotificationCompat.Builder(getContext(), CHANNEL_ID)
                .setContentTitle(currentTitle.isEmpty() ? "Cpz Music" : currentTitle)
                .setContentText(currentArtist.isEmpty() ? "Ready" : currentArtist)
                .setSubText(currentAlbum.isEmpty() ? null : currentAlbum)
                .setSmallIcon(android.R.drawable.ic_media_play)
                .setContentIntent(contentIntent)
                .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
                .setOnlyAlertOnce(true)
                .setOngoing(true)
                .addAction(
                    android.R.drawable.ic_media_previous, "Previous",
                    broadcast(MediaActionReceiver.ACTION_PREVIOUS, 1))
                .addAction(
                    isPlaying ? android.R.drawable.ic_media_pause : android.R.drawable.ic_media_play,
                    isPlaying ? "Pause" : "Play",
                    broadcast(MediaActionReceiver.ACTION_PLAY_PAUSE, 2))
                .addAction(
                    android.R.drawable.ic_media_next, "Next",
                    broadcast(MediaActionReceiver.ACTION_NEXT, 3))
                .setStyle(new MediaStyle()
                    .setMediaSession(mediaSession.getSessionToken())
                    .setShowActionsInCompactView(0, 1, 2));

        if (artwork != null) builder.setLargeIcon(artwork);
        return builder.build();
    }

    private PendingIntent broadcast(String action, int requestCode) {
        Intent intent = new Intent(action);
        // Explicitly packaged, so no other app can receive these control intents.
        intent.setPackage(getContext().getPackageName());
        return PendingIntent.getBroadcast(
            getContext(), requestCode, intent,
            PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
    }

    private void createNotificationChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return;
        NotificationChannel channel = new NotificationChannel(
            CHANNEL_ID, "Media Controls", NotificationManager.IMPORTANCE_LOW);
        channel.setDescription("Shows the current track with playback controls");
        channel.setShowBadge(false);
        NotificationManager manager =
            (NotificationManager) getContext().getSystemService(Context.NOTIFICATION_SERVICE);
        if (manager != null) manager.createNotificationChannel(channel);
    }

    @Override
    protected void handleOnDestroy() {
        if (mediaSession != null) {
            mediaSession.setActive(false);
            mediaSession.release();
        }
        instance = null;
    }
}
