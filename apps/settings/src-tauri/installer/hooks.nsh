; The dock (navalia.exe) is a bundled sidecar, not the Tauri main binary, so the
; installer stops and starts it itself.

; Quits the dock, gives the Windows taskbar back and waits for every
; navalia.exe (dock and watchdog) to exit, so its file can be replaced.
!macro NAVALIA_STOP_DOCK
  ${If} ${FileExists} "$INSTDIR\navalia.exe"
    ExecWait '"$INSTDIR\navalia.exe" --restore'
    StrCpy $R8 0
    ${Do}
      nsis_tauri_utils::FindProcessCurrentUser "navalia.exe"
      Pop $R9
      ${If} $R9 <> 0
        ${Break}
      ${EndIf}
      ${If} $R8 >= 50
        ; Still running after 5 s: force it, then restore the taskbar the
        ; killed watchdog can no longer restore.
        nsis_tauri_utils::KillProcessCurrentUser "navalia.exe"
        Pop $R9
        Sleep 300
        ExecWait '"$INSTDIR\navalia.exe" --restore'
        ${Break}
      ${EndIf}
      Sleep 100
      IntOp $R8 $R8 + 1
    ${Loop}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro NAVALIA_STOP_DOCK
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Updates keep whatever the user chose in Settings.
  ${If} $UpdateMode <> 1
    ExecWait '"$INSTDIR\navalia.exe" --autostart on'
  ${EndIf}
  ; Unelevated, even if the installer was run as administrator.
  nsis_tauri_utils::RunAsUser "$INSTDIR\navalia.exe" ""
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro NAVALIA_STOP_DOCK
  ; Removes the logon task (and an older Run entry), which would otherwise
  ; keep pointing at the deleted navalia.exe. Updates keep it.
  ${If} $UpdateMode <> 1
  ${AndIf} ${FileExists} "$INSTDIR\navalia.exe"
    ExecWait '"$INSTDIR\navalia.exe" --autostart off'
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    SetShellVarContext current
    ; The icon cache is always disposable.
    RmDir /r "$LOCALAPPDATA\Navalia"
    ; Where Navalia remembers that it starts with Windows.
    DeleteRegKey HKCU "Software\Navalia"
    ${If} $DeleteAppDataCheckboxState = 1
      RmDir /r "$APPDATA\Navalia"
    ${EndIf}
  ${EndIf}
!macroend
