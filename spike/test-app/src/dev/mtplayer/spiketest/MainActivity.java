package dev.mtplayer.spiketest;

import android.app.Activity;
import android.os.Build;
import android.os.Bundle;
import android.provider.Settings;
import android.util.Log;

import java.io.File;
import java.io.FileOutputStream;
import java.nio.charset.StandardCharsets;

/** Writes the identity values this app process sees to files/identity.txt, then exits. */
public class MainActivity extends Activity {
    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        StringBuilder sb = new StringBuilder();
        add(sb, "Build.MODEL", Build.MODEL);
        add(sb, "Build.BRAND", Build.BRAND);
        add(sb, "Build.DEVICE", Build.DEVICE);
        add(sb, "Build.MANUFACTURER", Build.MANUFACTURER);
        add(sb, "Build.PRODUCT", Build.PRODUCT);
        add(sb, "Build.FINGERPRINT", Build.FINGERPRINT);
        add(sb, "Build.ID", Build.ID);
        add(sb, "Build.SERIAL", Build.SERIAL);
        String serial;
        try {
            serial = Build.getSerial();
        } catch (SecurityException e) {
            serial = e.getClass().getSimpleName();
        }
        add(sb, "Build.getSerial", serial);
        add(sb, "ANDROID_ID", Settings.Secure.getString(getContentResolver(), Settings.Secure.ANDROID_ID));
        String text = sb.toString();
        try (FileOutputStream out = new FileOutputStream(new File(getFilesDir(), "identity.txt"))) {
            out.write(text.getBytes(StandardCharsets.UTF_8));
        } catch (Exception e) {
            Log.e("MtpSpike", "write failed", e);
        }
        Log.i("MtpSpike", text);
        finish();
    }

    private static void add(StringBuilder sb, String key, String value) {
        sb.append(key).append('=').append(value).append('\n');
    }
}
