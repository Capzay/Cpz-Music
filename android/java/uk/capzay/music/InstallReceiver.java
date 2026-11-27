package uk.capzay.music;

import android.app.PendingIntent;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageInstaller;
import android.util.Log;

import java.io.File;
import java.io.FileInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;

/**
 * Installs an update the Rust side downloaded from the server. Android asks the
 * user first (and, the first time, to allow installs from this app), then
 * replaces and restarts the app.
 */
public class InstallReceiver extends BroadcastReceiver {

    static void install(Context context, File apk) throws IOException {
        PackageInstaller installer = context.getPackageManager().getPackageInstaller();
        PackageInstaller.SessionParams params =
                new PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL);
        params.setAppPackageName(context.getPackageName());
        int id = installer.createSession(params);
        try (PackageInstaller.Session session = installer.openSession(id)) {
            try (InputStream in = new FileInputStream(apk);
                 OutputStream out = session.openWrite("update.apk", 0, apk.length())) {
                byte[] buffer = new byte[1 << 16];
                int n;
                while ((n = in.read(buffer)) > 0) out.write(buffer, 0, n);
                session.fsync(out);
            }
            Intent done = new Intent(context, InstallReceiver.class);
            // Mutable: the installer fills in the status.
            PendingIntent pending = PendingIntent.getBroadcast(context, id, done,
                    PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_MUTABLE);
            session.commit(pending.getIntentSender());
        }
        apk.delete();
    }

    @Override
    public void onReceive(Context context, Intent intent) {
        int status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE);
        if (status == PackageInstaller.STATUS_PENDING_USER_ACTION) {
            @SuppressWarnings("deprecation") // the typed overload needs API 33
            Intent confirm = intent.getParcelableExtra(Intent.EXTRA_INTENT);
            if (confirm != null) {
                confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
                context.startActivity(confirm);
            }
        } else if (status != PackageInstaller.STATUS_SUCCESS) {
            Log.w(MainActivity.TAG, "update not installed: "
                    + intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE));
        }
    }
}
