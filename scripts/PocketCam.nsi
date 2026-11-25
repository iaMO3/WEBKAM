Unicode true
Name "WEBKAM 1.0.1"
OutFile "${OUTPUT}"
InstallDir "$PROGRAMFILES64\PocketCam"
RequestExecutionLevel admin
SetCompressor /SOLID lzma
ShowInstDetails show
SetOverwrite on

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "nsDialogs.nsh"
!include "x64.nsh"
!include "WinVer.nsh"

!define MUI_ABORTWARNING
!define MUI_ICON "${ROOT}\assets\pocketcam-setup.ico"
!define MUI_UNICON "${ROOT}\assets\pocketcam-app.ico"
!define MUI_WELCOMEFINISHPAGE_BITMAP "${ROOT}\assets\pocketcam-wizard.bmp"
!define MUI_WELCOMEPAGE_TITLE "Welcome to WEBKAM Setup"
!define MUI_WELCOMEPAGE_TEXT "Install WEBKAM and its Windows virtual camera so you can use your phone in Discord and other video apps.$\r$\n$\r$\nClose WEBKAM before continuing."
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_FUNCTION LaunchWEBKAM
!define MUI_FINISHPAGE_RUN_TEXT "Open WEBKAM"
!define MUI_FINISHPAGE_TEXT "WEBKAM is installed. Connect your phone, then choose a connection in WEBKAM."
!insertmacro MUI_PAGE_WELCOME
Page custom UsbPage UsbPageLeave
Page custom ReviewPage
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Var AndroidBox
Var AppleBox
Var Android
Var Apple
Var UsbWarning

Function .onInit
    SetRegView 64
    StrCpy $Android 1
    StrCpy $Apple 1
    StrCpy $UsbWarning ""
    ${IfNot} ${RunningX64}
    ${OrIfNot} ${AtLeastWin10}
        MessageBox MB_OK|MB_ICONSTOP "WEBKAM requires Windows 11 on an Intel or AMD PC."
        Abort
    ${EndIf}
    ReadRegStr $0 HKLM "SOFTWARE\Microsoft\Windows NT\CurrentVersion" "CurrentBuild"
    IntCmp $0 22000 supported 0 supported
    MessageBox MB_OK|MB_ICONSTOP "WEBKAM requires Windows 11 on an Intel or AMD PC."
    Abort
supported:
FunctionEnd

Function UsbPage
    !insertmacro MUI_HEADER_TEXT "Phone USB support" "Choose which USB components to install."
    nsDialogs::Create 1018
    Pop $0
    ${If} $0 == error
        Abort
    ${EndIf}
    ${NSD_CreateCheckbox} 0 5u 100% 14u "Android USB"
    Pop $AndroidBox
    ${NSD_SetState} $AndroidBox $Android
    ${NSD_CreateLabel} 12u 21u 93% 24u "Downloads Google Platform-Tools and the Google USB driver. Other Android models may require a manufacturer driver. USB tethering uses Windows drivers."
    Pop $0
    ${NSD_CreateCheckbox} 0 53u 100% 14u "iPhone USB"
    Pop $AppleBox
    ${NSD_SetState} $AppleBox $Apple
    ${NSD_CreateLabel} 12u 69u 93% 23u "Downloads and installs Apple Mobile Device Support if needed. iPhone USB uses Personal Hotspot."
    Pop $0
    ${NSD_CreateLink} 0 103u 100% 14u "Android SDK terms"
    Pop $0
    ${NSD_OnClick} $0 OpenAndroidTerms
    ${NSD_CreateLabel} 0 121u 100% 26u "Click Next to continue with your choices and accept the Android SDK terms if Android USB is selected. Downloads require internet."
    Pop $0
    nsDialogs::Show
FunctionEnd

Function OpenAndroidTerms
    Pop $0
    ExecShell "open" "https://developer.android.com/studio/terms"
FunctionEnd

Function UsbPageLeave
    ${NSD_GetState} $AndroidBox $Android
    ${NSD_GetState} $AppleBox $Apple
FunctionEnd

Function ReviewPage
    !insertmacro MUI_HEADER_TEXT "Ready to install" "Review what Setup will install."
    nsDialogs::Create 1018
    Pop $0
    ${If} $0 == error
        Abort
    ${EndIf}
    StrCpy $1 "WEBKAM and the Windows virtual camera"
    ${If} $Android == 1
        StrCpy $1 "$1$\r$\nAndroid USB support"
    ${EndIf}
    ${If} $Apple == 1
        StrCpy $1 "$1$\r$\niPhone USB support"
    ${EndIf}
    ${NSD_CreateLabel} 0 10u 100% 55u "$1"
    Pop $0
    ${NSD_CreateLabel} 0 76u 100% 36u "Install location$\r$\n$INSTDIR"
    Pop $0
    GetDlgItem $0 $HWNDPARENT 1
    SendMessage $0 ${WM_SETTEXT} 0 "STR:&Install"
    nsDialogs::Show
FunctionEnd

Section "WEBKAM"
    IfSilent 0 +2
    StrCpy $Android 0
    IfSilent 0 +2
    StrCpy $Apple 0
    InitPluginsDir
    SetOutPath "$PLUGINSDIR"
    File "${ROOT}\target\release\pocketcam.exe"
    File "${ROOT}\target\release\pocketcam_camera.dll"
    File /oname=WEBKAM.ico "${ROOT}\assets\pocketcam-app.ico"
    File "${ROOT}\THIRD-PARTY-NOTICES.txt"
    File "Setup.ps1"
    File "UsbSupport.ps1"

    DetailPrint "Installing WEBKAM and registering the Windows virtual camera..."
    nsExec::ExecToLog '"$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\Setup.ps1" -Packaged'
    Pop $0
    ${If} $0 != 0
        SetErrorLevel 1
        Abort "WEBKAM setup failed. Close WEBKAM if it is open, then retry."
    ${EndIf}
    WriteUninstaller "$INSTDIR\Uninstall.exe"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\PocketCam" "DisplayName" "WEBKAM"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\PocketCam" "DisplayVersion" "1.0.1"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\PocketCam" "DisplayIcon" "$INSTDIR\WEBKAM.ico"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\PocketCam" "UninstallString" '"$INSTDIR\Uninstall.exe"'
    WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\PocketCam" "NoModify" 1
    WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\PocketCam" "NoRepair" 1

    StrCpy $1 ""
    ${If} $Android == 1
        StrCpy $1 "-Android"
    ${EndIf}
    ${If} $Apple == 1
        StrCpy $1 "$1 -IPhone"
    ${EndIf}
    ${If} $1 != ""
        DetailPrint "Setting up the selected USB components. This can take a few minutes..."
        nsExec::ExecToLog '"$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\UsbSupport.ps1" -InstallDir "$INSTDIR" $1'
        Pop $0
        ${If} $0 == 3010
            StrCpy $UsbWarning "WEBKAM is installed. Windows needs a restart to finish setting up USB support."
        ${ElseIf} $0 != 0
            StrCpy $UsbWarning "WEBKAM is installed. A USB component failed. See Show details, then run Setup again to retry."
        ${EndIf}
    ${EndIf}
    ${If} $UsbWarning != ""
        MessageBox MB_OK|MB_ICONEXCLAMATION "$UsbWarning"
    ${EndIf}
SectionEnd

Function LaunchWEBKAM
    ExecShell "open" "$WINDIR\explorer.exe" '"$INSTDIR\pocketcam.exe"'
FunctionEnd

Section "Uninstall"
    SetRegView 64
    InitPluginsDir
    SetOutPath "$PLUGINSDIR"
    File "Setup.ps1"
    nsExec::ExecToLog '"$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\Setup.ps1" -Packaged -Uninstall'
    Pop $0
    ${If} $0 != 0
        Abort "Close WEBKAM, then run the uninstaller again."
    ${EndIf}
    Delete "$INSTDIR\platform-tools\adb.exe"
    Delete "$INSTDIR\platform-tools\AdbWinApi.dll"
    Delete "$INSTDIR\platform-tools\AdbWinUsbApi.dll"
    Delete "$INSTDIR\platform-tools\libwinpthread-1.dll"
    Delete "$INSTDIR\platform-tools\NOTICE.txt"
    Delete "$INSTDIR\platform-tools\source.properties"
    RMDir "$INSTDIR\platform-tools"
    Delete "$INSTDIR\Uninstall.exe"
    RMDir "$INSTDIR"
    DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\PocketCam"
SectionEnd
