package uk.capzay.music;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;

public class MediaActionReceiver extends BroadcastReceiver {
    public static final String ACTION_PLAY_PAUSE = "uk.capzay.music.PLAY_PAUSE";
    public static final String ACTION_NEXT       = "uk.capzay.music.NEXT";
    public static final String ACTION_PREVIOUS   = "uk.capzay.music.PREVIOUS";

    @Override
    public void onReceive(Context context, Intent intent) {
        if (intent == null || intent.getAction() == null) return;
        switch (intent.getAction()) {
            case ACTION_PLAY_PAUSE: MediaSessionPlugin.handleAction("play_pause"); break;
            case ACTION_NEXT:       MediaSessionPlugin.handleAction("next");       break;
            case ACTION_PREVIOUS:   MediaSessionPlugin.handleAction("previous");   break;
        }
    }
}
