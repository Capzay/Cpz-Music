package uk.capzay.music;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.pm.ServiceInfo;
import android.media.AudioAttributes;
import android.media.AudioFocusRequest;
import android.media.AudioManager;
import android.media.MediaMetadata;
import android.media.session.MediaSession;
import android.media.session.PlaybackState;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.os.PowerManager;
import android.util.Log;

/**
 * Keeps music playing with the screen off: a foreground service while a track
 * plays, with a media notification, lock screen and headset controls, audio
 * focus (pause for calls and other apps) and pause when headphones come out.
 *
 * The sound itself comes from Rust; this only mirrors what Rust says is playing
 * (update) and passes button presses back (MainActivity.nativeCommand).
 * Everything runs on the main thread.
 */
public class PlaybackService extends Service {
    private static final String CHANNEL = "playback";
    private static final int NOTIFICATION = 1;
    private static final Handler main = new Handler(Looper.getMainLooper());

    private static String title = "";
    private static String artist = "";
    private static boolean playing;
    private static MediaSession session;
    private static PlaybackService running;

    private static AudioFocusRequest focus;
    private static boolean resumeOnFocus;
    private static boolean noisyRegistered;
    private static PowerManager.WakeLock wakeLock;

    private static final BroadcastReceiver noisy = new BroadcastReceiver() {
        @Override
        public void onReceive(Context context, Intent intent) {
            MainActivity.nativeCommand("pause");
        }
    };

    /** From Rust: what is playing now. An empty title means nothing is. */
    static void update(Context context, String newTitle, String newArtist, boolean nowPlaying) {
        main.post(() -> apply(context, newTitle, newArtist, nowPlaying));
    }

    private static void apply(Context context, String newTitle, String newArtist, boolean nowPlaying) {
        title = newTitle;
        artist = newArtist;
        playing = nowPlaying;

        if (title.isEmpty()) {
            release(context);
            context.stopService(new Intent(context, PlaybackService.class));
            return;
        }

        ensureSession(context);
        session.setMetadata(new MediaMetadata.Builder()
                .putString(MediaMetadata.METADATA_KEY_TITLE, title)
                .putString(MediaMetadata.METADATA_KEY_ARTIST, artist)
                .build());
        session.setPlaybackState(new PlaybackState.Builder()
                .setActions(PlaybackState.ACTION_PLAY | PlaybackState.ACTION_PAUSE
                        | PlaybackState.ACTION_PLAY_PAUSE | PlaybackState.ACTION_SKIP_TO_NEXT
                        | PlaybackState.ACTION_SKIP_TO_PREVIOUS)
                .setState(playing ? PlaybackState.STATE_PLAYING : PlaybackState.STATE_PAUSED,
                        PlaybackState.PLAYBACK_POSITION_UNKNOWN, playing ? 1f : 0f)
                .build());

        if (playing) {
            holdFocus(context);
            holdWakeLock(context);
            if (!noisyRegistered) {
                context.registerReceiver(noisy,
                        new IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY),
                        Context.RECEIVER_NOT_EXPORTED);
                noisyRegistered = true;
            }
        } else {
            releaseWakeLock();
            unregisterNoisy(context);
        }

        if (running != null) {
            running.refresh();
        } else if (playing) {
            try {
                context.startForegroundService(new Intent(context, PlaybackService.class));
            } catch (RuntimeException e) {
                // Android 12+ refuses when the app is in the background, e.g. when
                // another device starts playback here remotely.
                Log.w(MainActivity.TAG, "could not start the playback service", e);
            }
        }
    }

    private static void ensureSession(Context context) {
        if (session != null) return;
        session = new MediaSession(context, "cpz-music");
        session.setCallback(new MediaSession.Callback() {
            @Override public void onPlay() { MainActivity.nativeCommand("play"); }
            @Override public void onPause() { MainActivity.nativeCommand("pause"); }
            @Override public void onSkipToNext() { MainActivity.nativeCommand("next"); }
            @Override public void onSkipToPrevious() { MainActivity.nativeCommand("prev"); }
            @Override public void onStop() { MainActivity.nativeCommand("pause"); }
        });
        session.setActive(true);
    }

    private static void holdFocus(Context context) {
        if (focus != null) return;
        AudioManager audio = context.getSystemService(AudioManager.class);
        focus = new AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
                .setAudioAttributes(new AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_MEDIA)
                        .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                        .build())
                .setOnAudioFocusChangeListener(change -> {
                    switch (change) {
                        case AudioManager.AUDIOFOCUS_LOSS:
                            resumeOnFocus = false;
                            dropFocus(context);
                            MainActivity.nativeCommand("pause");
                            break;
                        case AudioManager.AUDIOFOCUS_LOSS_TRANSIENT:
                            resumeOnFocus = playing;
                            MainActivity.nativeCommand("pause");
                            break;
                        case AudioManager.AUDIOFOCUS_GAIN:
                            if (resumeOnFocus) MainActivity.nativeCommand("play");
                            resumeOnFocus = false;
                            break;
                        default:
                            // Ducking for short sounds is done by Android itself.
                            break;
                    }
                }, main)
                .build();
        audio.requestAudioFocus(focus);
    }

    private static void dropFocus(Context context) {
        if (focus == null) return;
        context.getSystemService(AudioManager.class).abandonAudioFocusRequest(focus);
        focus = null;
    }

    private static void holdWakeLock(Context context) {
        if (wakeLock == null) {
            wakeLock = context.getSystemService(PowerManager.class)
                    .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "cpz-music:playback");
            wakeLock.setReferenceCounted(false);
        }
        // Renewed on every update, so a stuck player cannot hold it for ever.
        wakeLock.acquire(6 * 60 * 60 * 1000L);
    }

    private static void releaseWakeLock() {
        if (wakeLock != null && wakeLock.isHeld()) wakeLock.release();
    }

    private static void unregisterNoisy(Context context) {
        if (!noisyRegistered) return;
        context.unregisterReceiver(noisy);
        noisyRegistered = false;
    }

    private static void release(Context context) {
        releaseWakeLock();
        unregisterNoisy(context);
        dropFocus(context);
        resumeOnFocus = false;
        if (session != null) {
            session.release();
            session = null;
        }
    }

    // ── The service ────────────────────────────────────────────────────────

    @Override
    public void onCreate() {
        super.onCreate();
        running = this;
        NotificationManager manager = getSystemService(NotificationManager.class);
        NotificationChannel channel = new NotificationChannel(
                CHANNEL, "Playback", NotificationManager.IMPORTANCE_LOW);
        channel.setShowBadge(false);
        manager.createNotificationChannel(channel);
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        String action = intent == null ? null : intent.getAction();
        if (action != null) MainActivity.nativeCommand(action);
        refresh();
        return START_NOT_STICKY;
    }

    @Override
    public void onDestroy() {
        running = null;
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    /** Shows the notification. While paused it can be swiped away. */
    private void refresh() {
        if (session == null) {
            stopSelf();
            return;
        }
        Notification notification = build();
        startForeground(NOTIFICATION, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK);
        if (!playing) stopForeground(STOP_FOREGROUND_DETACH);
    }

    private Notification build() {
        Intent open = new Intent(this, MainActivity.class)
                .setFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP);
        return new Notification.Builder(this, CHANNEL)
                .setSmallIcon(R.drawable.ic_notification)
                .setContentTitle(title)
                .setContentText(artist)
                .setContentIntent(PendingIntent.getActivity(this, 0, open, PendingIntent.FLAG_IMMUTABLE))
                .setOngoing(playing)
                .setVisibility(Notification.VISIBILITY_PUBLIC)
                .addAction(action(R.drawable.ic_prev, "Previous", "prev"))
                .addAction(playing
                        ? action(R.drawable.ic_pause, "Pause", "pause")
                        : action(R.drawable.ic_play, "Play", "play"))
                .addAction(action(R.drawable.ic_next, "Next", "next"))
                .setStyle(new Notification.MediaStyle()
                        .setMediaSession(session.getSessionToken())
                        .setShowActionsInCompactView(0, 1, 2))
                .build();
    }

    private Notification.Action action(int icon, String label, String command) {
        Intent intent = new Intent(this, PlaybackService.class).setAction(command);
        PendingIntent pending = PendingIntent.getForegroundService(
                this, command.hashCode(), intent, PendingIntent.FLAG_IMMUTABLE);
        return new Notification.Action.Builder(
                android.graphics.drawable.Icon.createWithResource(this, icon), label, pending).build();
    }
}
