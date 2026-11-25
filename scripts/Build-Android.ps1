param(
    [Parameter(Mandatory)][string]$BuildTools,
    [Parameter(Mandatory)][string]$PlatformJar,
    [Parameter(Mandatory)][string]$Jdk,
    [string]$SigningDirectory = (Join-Path $env:LOCALAPPDATA 'PocketCam/AndroidSigning')
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$source = Join-Path $root 'mobile/android'
$stage = Join-Path ([IO.Path]::GetTempPath()) ('PocketCamAndroid-' + [guid]::NewGuid())
$classes = New-Item -ItemType Directory -Force "$stage/classes"
$dex = New-Item -ItemType Directory "$stage/dex"
$assets = New-Item -ItemType Directory "$stage/assets"
Copy-Item (Join-Path $root 'mobile/home.html') $assets
$env:JAVA_HOME = $Jdk
$env:Path = "$Jdk/bin;$env:Path"
try {
    & "$BuildTools/aapt2.exe" compile --dir "$source/res" -o "$stage/resources.zip"
    & "$BuildTools/aapt2.exe" link -I $PlatformJar --manifest "$source/AndroidManifest.xml" -A $assets -o "$stage/unsigned.apk" "$stage/resources.zip"
    $javaSources = Get-ChildItem -LiteralPath $source -Filter '*.java' -File | Select-Object -ExpandProperty FullName
    & "$Jdk/bin/javac.exe" -encoding UTF-8 --release 8 -cp $PlatformJar -d $classes $javaSources
    & "$Jdk/bin/jar.exe" cf "$stage/classes.jar" -C $classes .
    & "$Jdk/bin/java.exe" -cp "$BuildTools/lib/d8.jar" com.android.tools.r8.D8 --release --min-api 26 --lib $PlatformJar --output $dex "$stage/classes.jar"
    & "$Jdk/bin/jar.exe" uf "$stage/unsigned.apk" -C $dex classes.dex
    & "$BuildTools/zipalign.exe" -p -f 4 "$stage/unsigned.apk" "$stage/aligned.apk"

    $sign = New-Item -ItemType Directory -Force $SigningDirectory
    $key = Join-Path $sign 'PocketCam.p12'
    $secret = Join-Path $sign 'password.txt'
    if (!(Test-Path $key) -and !(Test-Path $secret)) {
        $random = New-Object byte[] 32
        $rng = [Security.Cryptography.RandomNumberGenerator]::Create()
        try { $rng.GetBytes($random) } finally { $rng.Dispose() }
        [IO.File]::WriteAllText($secret, [Convert]::ToBase64String($random))
        $env:POCKETCAM_APK_PASSWORD = [IO.File]::ReadAllText($secret)
        & "$Jdk/bin/keytool.exe" -genkeypair -alias pocketcam -keystore $key -storetype PKCS12 -storepass:env POCKETCAM_APK_PASSWORD -keyalg RSA -keysize 3072 -validity 9000 -dname 'CN=PocketCam Local Android Release'
    }
    if (!(Test-Path $key) -or !(Test-Path $secret)) { throw 'Restore the existing Android signing key and password to sign updates.' }
    $env:POCKETCAM_APK_PASSWORD = [IO.File]::ReadAllText($secret)
    $out = Join-Path $root 'dist/PocketCam-Android.apk'
    & "$Jdk/bin/java.exe" -jar "$BuildTools/lib/apksigner.jar" sign --ks $key --ks-pass env:POCKETCAM_APK_PASSWORD --ks-key-alias pocketcam --v4-signing-enabled false --out $out "$stage/aligned.apk"
    & "$Jdk/bin/java.exe" -jar "$BuildTools/lib/apksigner.jar" verify --verbose $out
} finally {
    Remove-Item Env:POCKETCAM_APK_PASSWORD -ErrorAction SilentlyContinue
    $resolvedStage = (Resolve-Path $stage).Path
    if ([IO.Path]::GetDirectoryName($resolvedStage) -eq [IO.Path]::GetTempPath().TrimEnd('\') -and [IO.Path]::GetFileName($resolvedStage) -like 'PocketCamAndroid-*') {
        Remove-Item -LiteralPath $resolvedStage -Recurse -Force
    }
}
