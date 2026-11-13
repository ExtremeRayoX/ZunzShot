!macro NSIS_HOOK_POSTINSTALL
  CreateDirectory "$APPDATA\com.zunzshot.desktop"
  ${If} $LANGUAGE == 3082
  ${OrIf} $LANGUAGE == 1034
    FileOpen $0 "$APPDATA\com.zunzshot.desktop\installer_lang.txt" w
    FileWrite $0 "es"
    FileClose $0
  ${Else}
    FileOpen $0 "$APPDATA\com.zunzshot.desktop\installer_lang.txt" w
    FileWrite $0 "en"
    FileClose $0
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$APPDATA\com.zunzshot.desktop\installer_lang.txt"
!macroend
