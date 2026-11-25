package app.pocketcam.companion;

import android.app.Activity;
import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.os.Build;
import android.os.IBinder;
import android.os.ResultReceiver;

public class FloatingCameraService extends Service {
    private static final String CHANNEL = "pocketcam_camera";

    @Override public IBinder onBind(Intent intent) { return null; }

    @Override public int onStartCommand(Intent intent, int flags, int startId) {
        if (intent == null) {
            stopSelf(startId);
            return START_NOT_STICKY;
        }
        ResultReceiver ready = intent.getParcelableExtra("ready");
        try {
            NotificationManager notifications = getSystemService(NotificationManager.class);
            notifications.createNotificationChannel(new NotificationChannel(
                CHANNEL, "WEBKAM camera", NotificationManager.IMPORTANCE_LOW));
            PendingIntent open = PendingIntent.getActivity(this, 0,
                new Intent(this, MainActivity.class), PendingIntent.FLAG_IMMUTABLE);
            Notification notification = new Notification.Builder(this, CHANNEL)
                .setSmallIcon(android.R.drawable.ic_menu_camera)
                .setContentTitle("WEBKAM camera is active")
                .setContentText("Tap to return to WEBKAM")
                .setContentIntent(open)
                .setOngoing(true)
                .build();
            if (Build.VERSION.SDK_INT >= 30) {
                startForeground(41, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_CAMERA);
            } else {
                startForeground(41, notification);
            }
            if (ready != null) ready.send(Activity.RESULT_OK, null);
        } catch (RuntimeException error) {
            if (ready != null) ready.send(Activity.RESULT_CANCELED, null);
            stopSelf(startId);
        }
        return START_NOT_STICKY;
    }
}
