<#
.SYNOPSIS
  Reads every identity value the spike tracks from one running instance and
  returns it as an ordered hashtable (pipe it to ConvertTo-Json to save it).

.DESCRIPTION
  Needs adb root on the instance (run Invoke-Adb -s <serial> root first, or
  pass -Root). Read-only: it changes nothing in the guest except temp files in
  /data/local/tmp and, with -App, launching the stock WebView shell app.

  Values:
  - system properties: model/brand/device/manufacturer/name per partition,
    ro.build.fingerprint, ro.serialno, ro.boot.serialno
  - android_id as shell sees it (the system value in settings_secure.xml), and
    the per-app SSAID store /data/system/users/0/settings_ssaid.xml (the userkey
    is shown only as a sha256 prefix)
  - telephony for the active SIM: IMEI (iphonesubinfo tx 1), IMSI (tx 8), ICCID
    (tx 13), line 1 number (tx 16), plus every siminfo row (one per SIM ever seen)
  - MAC addresses: eth0, wlan0 and the Wi-Fi framework's MAC
  - with -App: the user agent the stock WebView shell app (an app process forked
    from zygote) builds from Build.MODEL and Build.ID, read with uiautomator;
    and, if the spike test app dev.mtplayer.spiketest is installed, the app.*
    lines it writes (Build.*, Build.getSerial(), its ANDROID_ID = its SSAID).

.EXAMPLE
  pwsh spike/read-identity.ps1 emulator-5610 -App
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory, Position = 0)][string]$Serial,
    [switch]$App,
    [switch]$Root
)

. (Join-Path $PSScriptRoot 'spike-env.ps1')

if ($Root) { Invoke-Adb -s $Serial root | Out-Null; Start-Sleep 2; Invoke-Adb -s $Serial wait-for-device }

$props = @('ro.product.model', 'ro.product.brand', 'ro.product.device', 'ro.product.manufacturer', 'ro.product.name',
           'ro.build.fingerprint', 'ro.serialno', 'ro.boot.serialno')
foreach ($part in 'system', 'vendor', 'product', 'odm', 'system_ext') {
    foreach ($f in 'model', 'brand', 'device', 'manufacturer') { $props += "ro.product.$part.$f" }
}

# One adb round trip: key=value lines.
$dev = @"
for p in $($props -join ' '); do echo "prop.`$p=`$(getprop `$p)"; done
echo "android_id=`$(settings get secure android_id)"
f=/data/system/users/0/settings_ssaid.xml
if [ -f `$f ]; then
  echo "ssaid.file=present"
  abx2xml `$f - | grep -o 'name="userkey" value="[^"]*"' | sed 's/.*value="//;s/"//' | sha256sum | cut -c1-12 | sed 's/^/ssaid.userkey.sha256=/'
  abx2xml `$f - | grep -v userkey | grep -o 'value="[^"]*" package="[^"]*"' | sed 's/value="\([^"]*\)" package="\([^"]*\)"/ssaid.app.\2=\1/'
else echo "ssaid.file=absent"; fi
echo "imei.raw=`$(service call iphonesubinfo 1 s16 com.android.shell | tr -d '\n')"
echo "imsi.raw=`$(service call iphonesubinfo 8 s16 com.android.shell | tr -d '\n')"
echo "iccid.raw=`$(service call iphonesubinfo 13 s16 com.android.shell | tr -d '\n')"
echo "line.raw=`$(service call iphonesubinfo 16 s16 com.android.shell | tr -d '\n')"
content query --uri content://telephony/siminfo --projection _id:icc_id:number:imsi:sim_id 2>/dev/null | sed 's/^Row: /siminfo.row/;s/ _id=/=_id=/'
for i in eth0 wlan0; do echo "mac.`$i=`$(cat /sys/class/net/`$i/address 2>/dev/null)"; done
echo "mac.wifi.framework=`$(dumpsys wifi 2>/dev/null | grep -m1 -o 'MAC: [0-9a-f:]*' | cut -c6-)"
"@
$out = Invoke-Adb -s $Serial shell ($dev -replace "`r", '')

# 'service call' prints a Parcel dump; the string is in the quoted columns.
function ConvertFrom-Parcel([string]$Raw) { (([regex]::Matches($Raw, "'([^']*)'") | ForEach-Object { $_.Groups[1].Value }) -join '') -replace '[^0-9A-Za-z+]', '' }

$r = [ordered]@{ serial = $Serial; read = (Get-Date).ToString('o') }
foreach ($line in $out) {
    if ($line -notmatch '^([^=]+)=(.*)$') { continue }
    $k = $Matches[1]; $v = $Matches[2].Trim()
    switch ($k) {
        'imei.raw' { $r['imei'] = ConvertFrom-Parcel $v }
        'imsi.raw' { $r['imsi'] = ConvertFrom-Parcel $v }
        'iccid.raw' { $r['sim.iccid'] = ConvertFrom-Parcel $v }
        'line.raw' { $r['line.number'] = ConvertFrom-Parcel $v }
        default    { $r[$k] = $v }
    }
}

if ($App) {
    # The WebView user agent carries Build.MODEL and Build.ID as an app process sees them.
    $html = "<html><body><p id=u>wait</p><script>document.getElementById('u').textContent='UA='+navigator.userAgent;</script></body></html>"
    $b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($html))
    $ua = Invoke-Adb -s $Serial shell ("am force-stop org.chromium.webview_shell; " +
        "am start -W -n org.chromium.webview_shell/.WebViewBrowserActivity -a android.intent.action.VIEW -d 'data:text/html;base64,$b64' >/dev/null; " +
        "for i in 1 2 3 4 5; do sleep 2; rm -f /data/local/tmp/ui.xml; uiautomator dump /data/local/tmp/ui.xml >/dev/null 2>&1; " +
        "grep -q 'UA=Mozilla' /data/local/tmp/ui.xml 2>/dev/null && break; done; " +
        "grep -o 'UA=[^`"]*' /data/local/tmp/ui.xml; am force-stop org.chromium.webview_shell")
    $r['app.webview.userAgent'] = ($ua | Select-Object -First 1)
    if ($r['app.webview.userAgent'] -match 'Android [0-9.]+; (.+) Build/([^;)]+)') {
        $r['app.webview.Build.MODEL'] = $Matches[1]; $r['app.webview.Build.ID'] = $Matches[2]
    }

    # The spike test app (spike/make-test-app.ps1), if installed: an ordinary app
    # writes Build.*, getSerial() and its own ANDROID_ID (its SSAID) to a file.
    $pkg = 'dev.mtplayer.spiketest'
    if ((Invoke-Adb -s $Serial shell "pm path $pkg 2>/dev/null") -match '^package:') {
        $f = "/data/data/$pkg/files/identity.txt"
        $lines = Invoke-Adb -s $Serial shell ("rm -f $f; am force-stop $pkg; am start -W -n $pkg/.MainActivity >/dev/null; " +
            "for i in 1 2 3 4 5 6 7 8 9 10; do [ -s $f ] && break; sleep 1; done; cat $f")
        foreach ($line in $lines) { if ($line -match '^([^=]+)=(.*)$') { $r["app.$($Matches[1])"] = $Matches[2].Trim() } }
        # The SettingsProvider writes the new SSAID (lazily created on that first
        # read) to settings_ssaid.xml; re-read the store so it is in this record.
        Start-Sleep -Seconds 3
        $ss = @(Invoke-Adb -s $Serial shell ("f=/data/system/users/0/settings_ssaid.xml; [ -f `$f ] && abx2xml `$f - | grep -o 'value=`"[^`"]*`" package=`"$pkg`"' | sed 's/value=`"\([^`"]*\)`".*/\1/'; " +
            "[ -f `$f ] && abx2xml `$f - | grep -o 'name=`"userkey`" value=`"[^`"]*`"' | sed 's/.*value=`"//;s/`"//' | sha256sum | cut -c1-12"))
        $r["ssaid.app.$pkg.afterAppRead"] = $ss[0]
        $r['ssaid.userkey.sha256.afterAppRead'] = $ss[1]
    } else { $r['app.testApp'] = 'not installed' }
}

$r
