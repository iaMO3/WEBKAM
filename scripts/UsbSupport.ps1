param(
    [Parameter(Mandatory)][string]$InstallDir,
    [switch]$Android,
    [switch]$IPhone,
    [switch]$VerifyPayloads,
    [string]$PayloadDirectory
)

$ErrorActionPreference = 'Stop'
$toolsArchive = 'platform-tools_r37.0.3-win.zip'
$driverArchive = 'usb_driver_r13-windows.zip'
$toolsHash = '89D108360A3BF2924D3BEA837239EDACCC22FD6FF5BEFD3787C4348A9FE4F09D'
$driverHash = '360B01D3DFB6C41621A3A64AE570DFAC2C9A40CCA1B5A1F136AE90D02F5E9E0B'
$work = New-Item -ItemType Directory -Path (Join-Path $PSScriptRoot ('usb-' + [Guid]::NewGuid()))

function Download([string]$Url, [string]$Name) {
    $path = Join-Path $work.FullName $Name
    if ($VerifyPayloads -and $PayloadDirectory -and (Test-Path (Join-Path $PayloadDirectory $Name))) {
        Copy-Item -LiteralPath (Join-Path $PayloadDirectory $Name) -Destination $path
    } else {
        & "$env:WINDIR\System32\curl.exe" --fail --silent --show-error --location --proto '=https' --retry 2 --connect-timeout 15 --max-time 300 --output $path $Url
        if ($LASTEXITCODE) { throw "Could not download $Name." }
    }
    return $path
}

function GoogleArchive([string]$Name, [string]$Hash, [string]$Destination) {
    $zip = Download "https://dl.google.com/android/repository/$Name" $Name
    if ((Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash -ne $Hash) { throw "Google checksum did not match for $Name." }
    Expand-Archive -LiteralPath $zip -DestinationPath $Destination
}

function AssertAppleSignature([string]$Path) {
    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.GetNameInfo('SimpleName', $false) -ne 'Apple Inc.') {
        throw "Apple's signature could not be verified on $(Split-Path -Leaf $Path)."
    }
}

function ExtractAppleMsi([string]$Setup) {
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.IO;

public static class PocketCamAppleCabinet {
    [DllImport("kernel32", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern IntPtr LoadLibraryEx(string path, IntPtr file, uint flags);
    [DllImport("kernel32", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern IntPtr FindResource(IntPtr module, string name, IntPtr kind);
    [DllImport("kernel32", SetLastError = true)]
    static extern IntPtr LoadResource(IntPtr module, IntPtr resource);
    [DllImport("kernel32", SetLastError = true)]
    static extern IntPtr LockResource(IntPtr data);
    [DllImport("kernel32", SetLastError = true)]
    static extern uint SizeofResource(IntPtr module, IntPtr resource);
    [DllImport("kernel32")]
    static extern bool FreeLibrary(IntPtr module);

    static IntPtr Check(IntPtr pointer) {
        if (pointer == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error());
        return pointer;
    }
    public static void Write(string executable, string cabinet) {
        // Load as data only. Never execute code from the Apple installer during extraction.
        IntPtr module = Check(LoadLibraryEx(executable, IntPtr.Zero, 0x22));
        try {
            IntPtr resource = Check(FindResource(module, "CABINET", new IntPtr(10)));
            uint size = SizeofResource(module, resource);
            if (size < 4 || size > 512 * 1024 * 1024) throw new IOException("Unexpected Apple cabinet size.");
            IntPtr bytes = Check(LockResource(Check(LoadResource(module, resource))));
            if (Marshal.ReadInt32(bytes) != 0x4643534D) throw new IOException("Apple cabinet header is missing.");
            using (var file = new FileStream(cabinet, FileMode.CreateNew, FileAccess.Write)) {
                byte[] buffer = new byte[1024 * 1024];
                for (int offset = 0; offset < size;) {
                    int count = Math.Min(buffer.Length, (int)size - offset);
                    Marshal.Copy(IntPtr.Add(bytes, offset), buffer, 0, count);
                    file.Write(buffer, 0, count);
                    offset += count;
                }
            }
        } finally { FreeLibrary(module); }
    }
}
'@
    $cab = Join-Path $work.FullName 'apple.cab'
    [PocketCamAppleCabinet]::Write($Setup, $cab)
    $name = 'AppleMobileDeviceSupport64.msi'
    & "$env:WINDIR\System32\expand.exe" $cab "-F:$name" $work.FullName | Out-Host
    $msi = Join-Path $work.FullName $name
    if ($LASTEXITCODE -or !(Test-Path -LiteralPath $msi)) { throw 'Apple USB support could not be extracted.' }
    AssertAppleSignature $msi
    return $msi
}

function InstallAndroidSupport {
    Write-Host 'Downloading Android Platform-Tools and the Google USB driver...'
    GoogleArchive $toolsArchive $toolsHash $work.FullName
    GoogleArchive $driverArchive $driverHash $work.FullName
    $platform = Join-Path $work.FullName 'platform-tools'
    $driver = Join-Path $work.FullName 'usb_driver\android_winusb.inf'
    if (!(Test-Path -LiteralPath $driver)) { throw 'The Google USB driver INF is missing.' }
    $catalog = Join-Path $work.FullName 'usb_driver\androidwinusba64.cat'
    if ((Get-AuthenticodeSignature -LiteralPath $catalog).Status -ne 'Valid') { throw 'Windows could not verify the Google driver catalog.' }
    & (Join-Path $platform 'adb.exe') version
    if ($LASTEXITCODE) { throw 'Android Platform-Tools could not start.' }
    if ($VerifyPayloads) { return }
    $dest = New-Item -ItemType Directory -Force (Join-Path $InstallDir 'platform-tools')
    foreach ($name in @('adb.exe', 'AdbWinApi.dll', 'AdbWinUsbApi.dll', 'libwinpthread-1.dll', 'NOTICE.txt', 'source.properties')) {
        $source = Join-Path $platform $name
        $target = Join-Path $dest.FullName $name
        if (!(Test-Path -LiteralPath $target) -or (Get-FileHash $source).Hash -ne (Get-FileHash $target).Hash) {
            Copy-Item -LiteralPath $source -Destination $target -Force
        }
    }
    & "$env:WINDIR\System32\pnputil.exe" /add-driver $driver /install
    if ($LASTEXITCODE -notin @(0, 259, 3010)) { throw "Windows could not stage the Google USB driver (code $LASTEXITCODE)." }
}

function InstallIPhoneSupport {
    if (!$VerifyPayloads -and
        (Get-Service 'Apple Mobile Device Service' -ErrorAction SilentlyContinue) -and
        (Get-ChildItem "$env:WINDIR\System32\DriverStore\FileRepository\netaapl64.inf_amd64_*" -Directory -ErrorAction SilentlyContinue)) {
        Write-Host 'Apple Mobile Device Support and USB Ethernet are already installed.'
        return
    }
    Write-Host 'Downloading Apple Mobile Device Support from Apple...'
    $setup = Download 'https://www.apple.com/itunes/download/win64' 'iTunes64Setup.exe'
    AssertAppleSignature $setup
    $msi = ExtractAppleMsi $setup
    if ($VerifyPayloads) { Write-Host 'Verified the signed Apple Mobile Device Support package.'; return }
    $log = Join-Path $env:ProgramData 'PocketCam-AppleUSB.log'
    $result = Start-Process "$env:WINDIR\System32\msiexec.exe" -ArgumentList "/i `"$msi`" /qn /norestart /L*v `"$log`"" -Wait -PassThru -WindowStyle Hidden
    if ($result.ExitCode -eq 3010) { return 3010 }
    if ($result.ExitCode -ne 0) { throw "Apple Mobile Device Support could not be installed (code $($result.ExitCode)). Log: $log" }
}

$failed = $false
$restart = $false
try {
    if ($Android) {
        try { InstallAndroidSupport } catch { Write-Host $_.Exception.Message; $failed = $true }
    }
    if ($IPhone) {
        try { $restart = (InstallIPhoneSupport) -eq 3010 } catch { Write-Host $_.Exception.Message; $failed = $true }
    }
    if (!$VerifyPayloads -and ($Android -or $IPhone)) {
        & "$env:WINDIR\System32\pnputil.exe" /scan-devices
    }
} finally {
    $parent = (Resolve-Path -LiteralPath $PSScriptRoot).Path
    $resolved = (Resolve-Path -LiteralPath $work.FullName).Path
    if ([IO.Path]::GetDirectoryName($resolved) -eq $parent -and [IO.Path]::GetFileName($resolved) -like 'usb-*') {
        Remove-Item -LiteralPath $resolved -Recurse -Force -ErrorAction SilentlyContinue
    }
}
if ($failed) { exit 1 }
if ($restart) { exit 3010 }
exit 0
