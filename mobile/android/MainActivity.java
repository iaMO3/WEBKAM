package app.pocketcam.companion;

import android.Manifest;
import android.app.*;
import android.content.*;
import android.content.pm.PackageManager;
import android.content.res.Configuration;
import android.graphics.Rect;
import android.net.Uri;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.ResultReceiver;
import android.util.Rational;
import android.view.*;
import android.webkit.*;
import android.widget.*;
import java.util.Arrays;

public class MainActivity extends Activity {
    private static final String HOME = "file:///android_asset/home.html";
    private WebView web;
    private Button back;
    private Uri camera;
    private PermissionRequest pending;
    private boolean floatingSupported;
    private boolean startingFloating;
    private boolean visible;

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        LinearLayout layout = new LinearLayout(this);
        layout.setOrientation(LinearLayout.VERTICAL);
        layout.setOnApplyWindowInsetsListener((v, insets) -> {
            v.setPadding(insets.getSystemWindowInsetLeft(), insets.getSystemWindowInsetTop(),
                insets.getSystemWindowInsetRight(), insets.getSystemWindowInsetBottom());
            return insets;
        });
        back = new Button(this);
        back.setText("‹  Connection");
        back.setAllCaps(false);
        back.setOnClickListener(v -> home());
        layout.addView(back);
        web = new WebView(this);
        layout.addView(web, new LinearLayout.LayoutParams(-1, 0, 1));
        setContentView(layout);
        web.getSettings().setJavaScriptEnabled(true);
        web.getSettings().setAllowFileAccess(false);
        web.getSettings().setAllowContentAccess(false);
        web.getSettings().setMediaPlaybackRequiresUserGesture(false);
        floatingSupported = getPackageManager().hasSystemFeature(PackageManager.FEATURE_PICTURE_IN_PICTURE);
        if (floatingSupported) {
            web.getSettings().setUserAgentString(web.getSettings().getUserAgentString() + " PocketCamAndroid/PiP");
        }
        web.setWebViewClient(new WebViewClient() {
            @Override public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest req) {
                Uri url = req.getUrl();
                if (!req.isForMainFrame()) return !sameOrigin(url);
                if (camera == null && "pocketcam".equals(url.getScheme()) && HOME.equals(view.getUrl())) {
                    connect(url);
                    return true;
                }
                if ("pocketcam".equals(url.getScheme()) && view.getUrl() != null
                    && sameOrigin(Uri.parse(view.getUrl()))) {
                    if ("float".equals(url.getHost()) && req.hasGesture()) {
                        enterFloating("portrait".equals(url.getLastPathSegment()));
                    } else if ("float-stopped".equals(url.getHost())) {
                        stopFloatingService();
                    }
                    return true;
                }
                return !sameOrigin(url);
            }
            @Override public void onReceivedError(WebView view, WebResourceRequest req, WebResourceError error) {
                if (req.isForMainFrame() && camera != null) showFailure();
            }
            @Override public void onReceivedSslError(WebView view, SslErrorHandler handler, android.net.http.SslError error) {
                handler.cancel();
                if (camera != null) showFailure();
            }
        });
        web.setWebChromeClient(new WebChromeClient() {
            @Override public void onPermissionRequest(PermissionRequest request) {
                if (!sameOrigin(request.getOrigin()) || !Arrays.asList(request.getResources()).contains(PermissionRequest.RESOURCE_VIDEO_CAPTURE)) {
                    request.deny(); return;
                }
                if (checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) {
                    request.grant(new String[]{PermissionRequest.RESOURCE_VIDEO_CAPTURE});
                } else {
                    if (pending != null) pending.deny();
                    pending = request;
                    requestPermissions(new String[]{Manifest.permission.CAMERA}, 41);
                }
            }
            @Override public void onPermissionRequestCanceled(PermissionRequest request) {
                if (pending == request) pending = null;
            }
        });
        home();
    }
    private boolean sameOrigin(Uri url) {
        return camera != null && camera.getScheme().equals(url.getScheme())
            && camera.getHost().equals(url.getHost()) && camera.getPort() == url.getPort()
            && url.getUserInfo() == null;
    }
    private static boolean localIP(String ip) {
        if (!ip.matches("[0-9]{1,3}(\\.[0-9]{1,3}){3}")) return false;
        String[] parts = ip.split("\\.");
        int[] n = new int[4];
        for (int i = 0; i < 4; i++) {
            n[i] = Integer.parseInt(parts[i]);
            if (n[i] > 255 || !String.valueOf(n[i]).equals(parts[i])) return false;
        }
        return n[0] == 10 || (n[0] == 192 && n[1] == 168) || (n[0] == 172 && n[1] >= 16 && n[1] <= 31);
    }
    private void connect(Uri link) {
        String ip = link.getLastPathSegment();
        if (ip == null) return;
        if ("usb".equals(link.getHost()) && ip.equals("127.0.0.1")) {
            camera = Uri.parse("http://127.0.0.1:47746");
        } else if (localIP(ip) && "setup".equals(link.getHost())) {
            startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse("http://" + ip + ":47744")));
            return;
        } else if (localIP(ip) && "camera".equals(link.getHost())) {
            camera = Uri.parse("https://" + ip + ":47745");
        } else return;
        back.setVisibility(View.VISIBLE);
        web.loadUrl(camera.toString());
    }
    private void showFailure() {
        home();
        new AlertDialog.Builder(this).setTitle("Couldn’t connect")
            .setMessage("Check the PC address and connection. For Wi-Fi or USB tethering, complete certificate setup, then open the camera again.")
            .setPositiveButton("OK", null).show();
    }
    private void home() {
        stopFloatingService();
        camera = null;
        if (pending != null) { pending.deny(); pending = null; }
        back.setVisibility(View.GONE);
        web.stopLoading();
        web.loadUrl(HOME);
    }
    @Override public void onRequestPermissionsResult(int code, String[] perms, int[] result) {
        super.onRequestPermissionsResult(code, perms, result);
        if (code == 41 && pending != null) {
            if (result.length > 0 && result[0] == PackageManager.PERMISSION_GRANTED && sameOrigin(pending.getOrigin()))
                pending.grant(new String[]{PermissionRequest.RESOURCE_VIDEO_CAPTURE});
            else pending.deny();
            pending = null;
        }
    }
    @Override public void onBackPressed() { if (camera != null) home(); else super.onBackPressed(); }

    private void dispatch(String event) {
        web.evaluateJavascript("window.dispatchEvent(new Event('" + event + "'))", null);
    }

    private void enterFloating(boolean portrait) {
        if (startingFloating || isInPictureInPictureMode()) return;
        if (!floatingSupported || !visible || checkSelfPermission(Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED) {
            dispatch("pocketcam-pip-failed");
            return;
        }
        startingFloating = true;
        ResultReceiver ready = new ResultReceiver(new Handler(Looper.getMainLooper())) {
            @Override protected void onReceiveResult(int result, Bundle data) {
                if (isDestroyed()) return;
                if (!startingFloating || !visible || camera == null) {
                    stopFloatingService();
                    return;
                }
                startingFloating = false;
                Rect bounds = new Rect();
                web.getGlobalVisibleRect(bounds);
                PictureInPictureParams.Builder params = new PictureInPictureParams.Builder()
                    .setAspectRatio(portrait ? new Rational(9, 16) : new Rational(16, 9));
                if (!bounds.isEmpty()) params.setSourceRectHint(bounds);
                try {
                    if (result == RESULT_OK && enterPictureInPictureMode(params.build())) return;
                } catch (RuntimeException ignored) {
                    // System PiP can be disabled by the user or a device policy.
                }
                stopFloatingService();
                dispatch("pocketcam-pip-failed");
            }
        };
        try {
            startForegroundService(new Intent(this, FloatingCameraService.class).putExtra("ready", ready));
        } catch (RuntimeException error) {
            startingFloating = false;
            dispatch("pocketcam-pip-failed");
        }
    }

    private void stopFloatingService() {
        startingFloating = false;
        stopService(new Intent(this, FloatingCameraService.class));
    }

    @Override public void onPictureInPictureModeChanged(boolean floating, Configuration configuration) {
        super.onPictureInPictureModeChanged(floating, configuration);
        back.setVisibility(floating || camera == null ? View.GONE : View.VISIBLE);
        dispatch(floating ? "pocketcam-pip-entered" : "pocketcam-pip-exited");
        if (!floating) stopFloatingService();
    }

    @Override public void onStart() {
        super.onStart();
        visible = true;
        web.onResume();
    }

    @Override public void onStop() {
        visible = false;
        dispatch("pocketcam-backgrounded");
        stopFloatingService();
        web.onPause();
        super.onStop();
    }

    @Override public void onDestroy() {
        stopFloatingService();
        web.destroy();
        super.onDestroy();
    }
}
