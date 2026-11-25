param([switch]$Uninstall, [switch]$Packaged)

$ErrorActionPreference = 'Stop'
$installDir = Join-Path $env:ProgramFiles 'PocketCam'
$shortcutPath = Join-Path ([Environment]::GetFolderPath('CommonPrograms')) 'WEBKAM.lnk'
$legacyShortcut = Join-Path ([Environment]::GetFolderPath('CommonPrograms')) 'PocketCam.lnk'
$appPath = Join-Path $installDir 'pocketcam.exe'
$iconPath = Join-Path $installDir 'WEBKAM.ico'
$clsid = '{659F03EB-0BAE-4D0A-972E-8B6A6F323807}'
$classKey = "HKLM:\SOFTWARE\Classes\CLSID\$clsid"
$oldClassKey = 'HKLM:\SOFTWARE\Classes\CLSID\{114A8121-77B2-460A-8D75-1C9CF0B0C614}'
$firewallName = 'PocketCam-iPhone-Local-Network'

$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    $script = $PSCommandPath.Replace("'", "''")
    $flag = if ($Uninstall) { ' -Uninstall' } else { '' }
    if ($Packaged) { $flag += ' -Packaged' }
    $command = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes("& '$script'$flag"))
    $setup = Start-Process -FilePath "$PSHOME\powershell.exe" -ArgumentList @(
        '-NoLogo', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', $command
    ) -Verb RunAs -Wait -PassThru
    if ($setup.ExitCode -eq 0 -and -not $Uninstall -and -not $Packaged) { Start-Process -FilePath $appPath }
    exit $setup.ExitCode
}

try {
    if (Get-Process -Name 'pocketcam' -ErrorAction SilentlyContinue) {
        throw 'Close the WEBKAM window before running setup.'
    }
    if ($Uninstall) {
        Remove-NetFirewallRule -Name $firewallName -ErrorAction SilentlyContinue
        foreach ($key in @($classKey, $oldClassKey)) {
            $serverKey = "$key\InprocServer32"
            if (Test-Path -LiteralPath $serverKey) { Remove-Item -LiteralPath $serverKey }
            if (Test-Path -LiteralPath $key) { Remove-Item -LiteralPath $key }
        }
        if (Test-Path -LiteralPath $legacyShortcut) { Remove-Item -LiteralPath $legacyShortcut }
        if (Test-Path -LiteralPath $shortcutPath) { Remove-Item -LiteralPath $shortcutPath }
        $notices = Join-Path $installDir 'THIRD-PARTY-NOTICES.txt'
        if (Test-Path -LiteralPath $notices) { Remove-Item -LiteralPath $notices }
        if (Test-Path -LiteralPath $appPath) { Remove-Item -LiteralPath $appPath }
        if (Test-Path -LiteralPath $iconPath) { Remove-Item -LiteralPath $iconPath }
        if (Test-Path -LiteralPath $installDir) {
            Get-ChildItem -LiteralPath $installDir -File -Filter 'pocketcam_camera-*.dll' | ForEach-Object {
                Remove-Item -LiteralPath $_.FullName -ErrorAction SilentlyContinue
            }
            if (-not (Get-ChildItem -LiteralPath $installDir -Force | Select-Object -First 1)) {
                Remove-Item -LiteralPath $installDir
            }
        }
        Write-Host 'WEBKAM was unregistered. Restart Windows if a camera DLL was still in use.'
        exit 0
    }

    $build = [int](Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion').CurrentBuild
    if ($build -lt 22000 -or $env:PROCESSOR_ARCHITECTURE -ne 'AMD64') {
        throw 'This WEBKAM build requires 64-bit Windows 11 on an Intel or AMD PC.'
    }
    $sourceExe = Join-Path $PSScriptRoot 'pocketcam.exe'
    $sourceDll = Join-Path $PSScriptRoot 'pocketcam_camera.dll'
    $dllVersion = (Get-FileHash -LiteralPath $sourceDll -Algorithm SHA256).Hash.Substring(0, 16)
    $dllPath = Join-Path $installDir "pocketcam_camera-$dllVersion.dll"
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
    if (-not (Test-Path -LiteralPath $dllPath)) {
        Copy-Item -LiteralPath $sourceDll -Destination $dllPath
    }
    if (Test-Path (Join-Path $PSScriptRoot 'THIRD-PARTY-NOTICES.txt')) { Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'THIRD-PARTY-NOTICES.txt') -Destination $installDir -Force }
    Copy-Item -LiteralPath $sourceExe -Destination $appPath -Force
    $sourceIcon = Join-Path $PSScriptRoot 'WEBKAM.ico'
    if (Test-Path -LiteralPath $sourceIcon) {
        Copy-Item -LiteralPath $sourceIcon -Destination $iconPath -Force
    }

    $serverKey = "$classKey\InprocServer32"
    New-Item -Path $classKey -Force | Out-Null
    New-Item -Path $serverKey -Force | Out-Null
    Set-Item -LiteralPath $classKey -Value 'WEBKAM Camera Source'
    Set-Item -LiteralPath $serverKey -Value $dllPath
    New-ItemProperty -LiteralPath $serverKey -Name 'ThreadingModel' -Value 'Both' -PropertyType String -Force | Out-Null

    $currentFirewall = Get-NetFirewallRule -Name $firewallName -ErrorAction SilentlyContinue
    if ($currentFirewall) {
        Set-NetFirewallRule -Name $firewallName -NewDisplayName 'WEBKAM iPhone connection' | Out-Null
        $currentFirewall | Get-NetFirewallPortFilter | Set-NetFirewallPortFilter -Protocol TCP -LocalPort 47744,47745 | Out-Null
    } else {
        $firewall = @{
            Name = $firewallName
            DisplayName = 'WEBKAM iPhone connection'
            Direction = 'Inbound'
            Action = 'Allow'
            Program = $appPath
            Protocol = 'TCP'
            LocalPort = 47744,47745
            RemoteAddress = 'LocalSubnet'
            Profile = 'Private'
        }
        New-NetFirewallRule @firewall | Out-Null
    }
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($shortcutPath)
    $shortcut.TargetPath = $appPath
    $shortcut.IconLocation = if (Test-Path -LiteralPath $iconPath) { "$iconPath,0" } else { "$appPath,0" }
    $shortcut.WorkingDirectory = $installDir
    $shortcut.Description = 'Use your iPhone or Android camera as a Windows webcam'
    $shortcut.Save()
    if (Test-Path -LiteralPath $legacyShortcut) { Remove-Item -LiteralPath $legacyShortcut }
    try {
        Add-Type -Namespace WEBKAM -Name Shell -MemberDefinition @"
[System.Runtime.InteropServices.DllImport("shell32.dll", CharSet = System.Runtime.InteropServices.CharSet.Unicode, ExactSpelling = true)]
public static extern void SHChangeNotify(int eventId, uint flags, string item, System.IntPtr unused);
"@
        # SHCNE_UPDATEITEM, with a Unicode path and asynchronous delivery.
        foreach ($path in @($appPath, $iconPath, $shortcutPath)) {
            if (Test-Path -LiteralPath $path) {
                [WEBKAM.Shell]::SHChangeNotify(0x2000, 0x3005, $path, [IntPtr]::Zero)
            }
        }
    } catch {
        Write-Warning 'Windows could not refresh the WEBKAM icon.'
    }
    Write-Host 'WEBKAM is installed.'
} catch {
    Write-Host $_.Exception.Message -ForegroundColor Red
    if (!$Packaged) { Read-Host 'Press Enter to close setup' }
    exit 1
}
