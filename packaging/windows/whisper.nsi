; Whisper installer for Windows 10 and 11: per user, no administrator rights.
;
; Built by `cargo xtask nsis`, which passes:
;   VERSION     e.g. 0.1.0
;   VIVERSION   e.g. 0.1.0.0 (Windows version resource)
;   STAGE       folder with whisper.exe, share\, LICENSE.txt, whisper.ico, sidebar.bmp, header.bmp
;   OUTFILE     where to write Whisper-Setup.exe
;   SIZE_KB     installed size for "Apps & features"
;
; Silent use: Whisper-Setup.exe /S           install or update (Whisper's own updater does this)
;             uninstall.exe /S [/PURGE]     remove; /PURGE also deletes the Whisper profile

Unicode true
ManifestDPIAware true
SetCompressor /SOLID lzma
RequestExecutionLevel user

!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Whisper"
!define REPO_URL "https://github.com/footprintsntrails/whisper-browser"

Name "Whisper"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\Whisper"
BrandingText "Whisper ${VERSION}"

VIProductVersion "${VIVERSION}"
VIAddVersionKey "ProductName" "Whisper"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "Whisper Setup"
VIAddVersionKey "CompanyName" "Whisper"
VIAddVersionKey "LegalCopyright" "MPL-2.0"

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"
!include "Sections.nsh"

!define MUI_ICON "${STAGE}\whisper.ico"
!define MUI_UNICON "${STAGE}\whisper.ico"
!define MUI_WELCOMEFINISHPAGE_BITMAP "${STAGE}\sidebar.bmp"
!define MUI_UNWELCOMEFINISHPAGE_BITMAP "${STAGE}\sidebar.bmp"
!define MUI_HEADERIMAGE
!define MUI_HEADERIMAGE_RIGHT
!define MUI_HEADERIMAGE_BITMAP "${STAGE}\header.bmp"
!define MUI_HEADERIMAGE_UNBITMAP "${STAGE}\header.bmp"
!define MUI_ABORTWARNING

!define MUI_WELCOMEPAGE_TITLE "Welcome to Whisper"
!define MUI_WELCOMEPAGE_TEXT "Whisper is a quiet browser built on Firefox: no telemetry, uBlock Origin from the start, a dark look with tabs on the side.$\r$\n$\r$\nWhisper uses your installed Firefox with a separate Whisper profile, so your normal Firefox stays as it is. If Firefox is missing, Whisper offers to install it from mozilla.org.$\r$\n$\r$\nIt installs for you only and needs no administrator rights."
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "${STAGE}\LICENSE.txt"
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_TITLE "Whisper is ready"
!define MUI_FINISHPAGE_TEXT "Whisper can't make you invisible. It just won't snitch.$\r$\n$\r$\nOn the first start you can bring over bookmarks, history and passwords from another browser."
!define MUI_FINISHPAGE_RUN "$INSTDIR\whisper.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Launch Whisper"
!define MUI_FINISHPAGE_LINK "Whisper on GitHub"
!define MUI_FINISHPAGE_LINK_LOCATION "${REPO_URL}"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!define MUI_COMPONENTSPAGE_NODESC
!define MUI_COMPONENTSPAGE_TEXT_TOP "Whisper will be removed. Your Whisper profile (bookmarks, history, passwords, settings) is kept unless you tick it below."
!insertmacro MUI_UNPAGE_COMPONENTS
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Section "Whisper" SecMain
  SectionIn RO
  ; An existing install means this is an update: keep the user's choice about the desktop icon.
  ReadRegStr $0 HKCU "${UNINSTALL_KEY}" "InstallLocation"

  SetOutPath "$INSTDIR"
  ; Updates can run while Whisper is open; a running program can be renamed but not replaced.
  Delete "$INSTDIR\whisper.exe.old"
  IfFileExists "$INSTDIR\whisper.exe" 0 +2
    Rename "$INSTDIR\whisper.exe" "$INSTDIR\whisper.exe.old"
  RMDir /r "$INSTDIR\share"
  File "${STAGE}\whisper.exe"
  File "${STAGE}\LICENSE.txt"
  SetOutPath "$INSTDIR\share"
  File /r "${STAGE}\share\*"
  SetOutPath "$INSTDIR"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateDirectory "$SMPROGRAMS\Whisper"
  CreateShortcut "$SMPROGRAMS\Whisper\Whisper.lnk" "$INSTDIR\whisper.exe" "" "$INSTDIR\whisper.exe" 0 SW_SHOWNORMAL "" "A quiet browser built on Firefox"
  CreateShortcut "$SMPROGRAMS\Whisper\Import from another browser.lnk" "$INSTDIR\whisper.exe" "--import" "$INSTDIR\whisper.exe" 0 SW_SHOWNORMAL "" "Bring bookmarks, history and passwords into Whisper"
  ${If} $0 == ""
    CreateShortcut "$DESKTOP\Whisper.lnk" "$INSTDIR\whisper.exe" "" "$INSTDIR\whisper.exe" 0 SW_SHOWNORMAL "" "A quiet browser built on Firefox"
  ${EndIf}

  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "Whisper"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\whisper.exe,0"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "Whisper"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "URLInfoAbout" "${REPO_URL}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "HelpLink" "${REPO_URL}/issues"
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" ${SIZE_KB}
SectionEnd

Section "un.Whisper" UnSecMain
  SectionIn RO
  Delete "$INSTDIR\whisper.exe"
  Delete "$INSTDIR\whisper.exe.old"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\uninstall.exe"
  RMDir /r "$INSTDIR\share"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\Whisper\Whisper.lnk"
  Delete "$SMPROGRAMS\Whisper\Import from another browser.lnk"
  RMDir "$SMPROGRAMS\Whisper"
  Delete "$DESKTOP\Whisper.lnk"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
SectionEnd

Section /o "un.Also delete my Whisper profile" UnSecProfile
  RMDir /r "$LOCALAPPDATA\Whisper"
SectionEnd

Function un.onInit
  ${un.GetParameters} $R0
  ClearErrors
  ${un.GetOptions} $R0 "/PURGE" $R1
  ${IfNot} ${Errors}
    !insertmacro SelectSection ${UnSecProfile}
  ${EndIf}
FunctionEnd
