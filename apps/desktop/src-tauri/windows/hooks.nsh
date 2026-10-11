; Offline kit: copy models-extra\ next to the install when the installer ships with one.
!macro NSIS_HOOK_POSTINSTALL
  IfFileExists "$EXEDIR\models-extra\pack.json" 0 +3
    CreateDirectory "$INSTDIR\models-extra"
    CopyFiles /SILENT "$EXEDIR\models-extra\*.*" "$INSTDIR\models-extra"
!macroend
