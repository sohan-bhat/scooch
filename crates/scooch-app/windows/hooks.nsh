Var ScoochWasRunning

!macro NSIS_HOOK_PREINSTALL
  ${If} $INSTDIR == "$LOCALAPPDATA\${PRODUCTNAME}"
    StrCpy $INSTDIR "$LOCALAPPDATA\Programs\${PRODUCTNAME}"
    SetOutPath $INSTDIR
  ${EndIf}
  Push $1
  InitPluginsDir
  FileOpen $1 "$PLUGINSDIR\end-icon.ps1" w
  FileWrite $1 `param([string]$$Folder)$\r$\n`
  FileWrite $1 `$$prefix = $$Folder.TrimEnd('\') + '\'$\r$\n`
  FileWrite $1 `Get-Process -Name scooch-app -ErrorAction SilentlyContinue | Where-Object { $$_.Path -and $$_.Path.StartsWith($$prefix, [StringComparison]::OrdinalIgnoreCase) } | Stop-Process -Force$\r$\n`
  FileClose $1
  Pop $1
  nsExec::Exec '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File "$PLUGINSDIR\end-icon.ps1" "$INSTDIR"'
  Pop $0
  StrCpy $ScoochWasRunning "no"
  nsExec::Exec 'taskkill /F /IM scoochw.exe'
  Pop $0
  ${If} $0 == 0
    StrCpy $ScoochWasRunning "yes"
    Sleep 500
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  nsExec::Exec '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\hooks.ps1" installed "$INSTDIR" $ScoochWasRunning'
  Pop $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    nsExec::Exec '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\hooks.ps1" removing "$INSTDIR" no'
    Pop $0
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    RmDir /r "$LOCALAPPDATA\Scooch"
  ${EndIf}
!macroend
