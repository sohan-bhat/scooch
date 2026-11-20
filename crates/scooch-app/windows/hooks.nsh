Var ScoochWasRunning

!macro NSIS_HOOK_PREINSTALL
  ${If} $INSTDIR == "$LOCALAPPDATA\${PRODUCTNAME}"
    StrCpy $INSTDIR "$LOCALAPPDATA\Programs\${PRODUCTNAME}"
    SetOutPath $INSTDIR
  ${EndIf}
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
