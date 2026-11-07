; Discord Bridge NSIS hooks.
; The helper service is the only privileged component; the installer
; (already elevated in perMachine mode) registers, upgrades and removes it.

!include "WinVer.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"

; A file that cannot be written must never be skipped: a skipped file means a
; mix of versions. Only Retry or Cancel are offered.
AllowSkipFiles off

; Restart the helper after an aborted upgrade so the installed version keeps
; working (the files were not changed).
!macro DB_RESTART_SERVICE
  nsExec::ExecToLog 'sc start DiscordBridgeHelper'
  Pop $9
!macroend

; Wait until the helper service is STOPPED (or absent). Older helpers return 0
; from --stop-engine even when the stop failed, so the state is checked here
; independently. Result in $8: "ok" or "timeout".
!macro DB_WAIT_SERVICE_STOPPED
  StrCpy $8 "timeout"
  StrCpy $7 0
  ${Do}
    nsExec::ExecToStack 'sc query DiscordBridgeHelper'
    Pop $6
    Pop $5
    ${If} $6 == 1060
      StrCpy $8 "ok" ; not installed
      ${Break}
    ${EndIf}
    nsExec::ExecToStack 'cmd /c sc query DiscordBridgeHelper | find "STOPPED"'
    Pop $6
    Pop $5
    ${If} $6 == 0
      StrCpy $8 "ok"
      ${Break}
    ${EndIf}
    IntOp $7 $7 + 1
    ${If} $7 >= 60
      ${Break}
    ${EndIf}
    Sleep 500
  ${Loop}
!macroend

; Telegram WS Proxy is an ordinary child process of Discord Bridge held in a
; kill-on-close Job Object, so it ends with the app (the installer closes the
; app first). Defensive: stop only a process running exactly our installed
; executable — never anything else that may use the same port.
!macro DB_STOP_TGPROXY
  nsExec::ExecToLog `powershell.exe -NoProfile -NonInteractive -Command "$$p = '$INSTDIR\tg-ws-proxy\discord-bridge-tg-ws-proxy.exe'; Get-CimInstance Win32_Process | Where-Object { $$_.ExecutablePath -eq $$p } | ForEach-Object { Stop-Process -Id $$_.ProcessId -Force -ErrorAction SilentlyContinue }"`
  Pop $9
!macroend

!macro NSIS_HOOK_PREINSTALL
  ${IfNot} ${AtLeastWin10}
    MessageBox MB_ICONSTOP "Discord Bridge требует Windows 10 или Windows 11 (x64)." /SD IDOK
    Abort
  ${EndIf}
  !insertmacro DB_STOP_TGPROXY
  ; Upgrade: stop the running helper (this also stops its own winws through
  ; the job object). Foreign zapret installations are not touched.
  ${If} ${FileExists} "$INSTDIR\discord-bridge-helper.exe"
    nsExec::ExecToLog '"$INSTDIR\discord-bridge-helper.exe" --stop-engine'
    Pop $0
    ${If} $0 == 3
      MessageBox MB_ICONSTOP "Не удалось остановить службу Discord Bridge Helper. Обновление прервано, установленная версия не изменена.$\n$\nПерезагрузите компьютер и запустите установку снова." /SD IDOK
      SetErrorLevel 3
      Abort
    ${ElseIf} $0 == 4
      !insertmacro DB_RESTART_SERVICE
      MessageBox MB_ICONSTOP "Движок обхода Discord Bridge (winws) не завершился. Обновление прервано, установленная версия не изменена.$\n$\nПерезагрузите компьютер и запустите установку снова." /SD IDOK
      SetErrorLevel 4
      Abort
    ${EndIf}
    !insertmacro DB_WAIT_SERVICE_STOPPED
    ${If} $8 != "ok"
      MessageBox MB_ICONSTOP "Служба Discord Bridge Helper не остановилась за 30 секунд. Обновление прервано, установленная версия не изменена.$\n$\nПерезагрузите компьютер и запустите установку снова." /SD IDOK
      SetErrorLevel 3
      Abort
    ${EndIf}
  ${EndIf}
  ; The previous Flowseal files are moved aside as a whole, never overwritten
  ; in place: WinDivert64.sys stays locked while the driver loaded from it is
  ; in memory (versions up to 0.1.0-alpha.7 ran winws from this folder). A
  ; rename is allowed for such a file; if even that fails, nothing has been
  ; changed yet and the user is asked to reboot. The helper deletes the old
  ; folder on its next start once nothing holds it.
  ${If} ${FileExists} "$INSTDIR\flowseal\*.*"
    ${GetTime} "" "L" $0 $1 $2 $3 $4 $5 $6
    StrCpy $R0 "$INSTDIR\flowseal.old-$2$1$0-$4$5$6"
    StrCpy $7 0
    ${Do}
      ClearErrors
      Rename "$INSTDIR\flowseal" "$R0"
      ${IfNot} ${Errors}
        ${Break}
      ${EndIf}
      IntOp $7 $7 + 1
      ${If} $7 >= 15
        ${Break}
      ${EndIf}
      Sleep 1000
    ${Loop}
    ${If} ${FileExists} "$INSTDIR\flowseal\*.*"
      !insertmacro DB_RESTART_SERVICE
      MessageBox MB_ICONSTOP "Файлы Flowseal в папке Discord Bridge заняты: их удерживает Windows (загруженный драйвер WinDivert) или другая программа обхода.$\n$\nОбновление прервано, ничего не изменено. Перезагрузите компьютер и запустите установку снова; если используется другая сборка Zapret, сначала закройте её." /SD IDOK
      SetErrorLevel 5
      Abort
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Registers the service, or reconfigures the existing one in place (it is
  ; never deleted during an upgrade), starts it and verifies it is running.
  nsExec::ExecToLog '"$INSTDIR\discord-bridge-helper.exe" --install-service'
  Pop $0
  ${If} $0 != 0
    MessageBox MB_ICONSTOP "Не удалось зарегистрировать или запустить службу Discord Bridge Helper (код $0). Без неё обход DPI не работает, поэтому установка не считается успешной. Файлы приложения установлены полностью; запустите установщик повторно или перезагрузите компьютер." /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}
!macroend

!macro DB_REMOVE_AUTOSTART_FOR ROOTKEY SUBKEY
  ; Delete the "Discord Bridge" Run value only if it points to our executable.
  ReadRegStr $2 ${ROOTKEY} "${SUBKEY}Software\Microsoft\Windows\CurrentVersion\Run" "Discord Bridge"
  ${If} $2 == '"$INSTDIR\discord-bridge.exe" --minimized'
  ${OrIf} $2 == '"$INSTDIR\discord-bridge.exe"'
  ${OrIf} $2 == '$INSTDIR\discord-bridge.exe --minimized'
  ${OrIf} $2 == '$INSTDIR\discord-bridge.exe'
    DeleteRegValue ${ROOTKEY} "${SUBKEY}Software\Microsoft\Windows\CurrentVersion\Run" "Discord Bridge"
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro DB_STOP_TGPROXY
  ; 1. TCP timestamps: offer to restore only a change Discord Bridge made and
  ;    that nobody changed since (the helper checks both).
  ${If} ${FileExists} "$INSTDIR\discord-bridge-helper.exe"
    nsExec::ExecToLog '"$INSTDIR\discord-bridge-helper.exe" --tcp-changed'
    Pop $0
    ${If} $0 == 10
      MessageBox MB_YESNO|MB_ICONQUESTION "Discord Bridge включил TCP timestamps в Windows. Вернуть исходное значение?" /SD IDNO IDNO +3
        nsExec::ExecToLog '"$INSTDIR\discord-bridge-helper.exe" --restore-tcp'
        Pop $0
    ${EndIf}
    ; 2. Stop our engine and remove our service only. On failure, abort so the
    ;    app is not left without its service in an undefined state.
    nsExec::ExecToLog '"$INSTDIR\discord-bridge-helper.exe" --uninstall-service'
    Pop $0
    ${If} $0 != 0
      MessageBox MB_ICONSTOP "Не удалось остановить и удалить службу Discord Bridge Helper (код $0: 3 — служба не остановилась, 4 — движок не завершился). Удаление прервано, ничего не удалено; перезагрузите компьютер и попробуйте ещё раз." /SD IDOK
      SetErrorLevel $0
      Abort
    ${EndIf}
  ${EndIf}
  ; 3. Autostart entries: the current (elevated) user and every loaded user
  ;    profile under HKEY_USERS, only values pointing to our executable.
  !insertmacro DB_REMOVE_AUTOSTART_FOR HKCU ""
  StrCpy $1 0
  db_hku_loop:
    EnumRegKey $3 HKU "" $1
    StrCmp $3 "" db_hku_done
    IntOp $1 $1 + 1
    !insertmacro DB_REMOVE_AUTOSTART_FOR HKU "$3\"
    Goto db_hku_loop
  db_hku_done:
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Helper state (verified Flowseal updates, generated lists, helper log).
  ; Resolved from the environment so the installer's shell context is untouched.
  ; Folders moved aside by upgrades (normally already removed by the helper).
  FindFirst $0 $1 "$INSTDIR\flowseal.old-*"
  ${DoWhile} $1 != ""
    RMDir /r /REBOOTOK "$INSTDIR\$1"
    FindNext $0 $1
  ${Loop}
  FindClose $0
  ReadEnvStr $1 "PROGRAMDATA"
  ${If} $1 != ""
    ; Engine copies may still be held by a loaded WinDivert driver: those are
    ; removed after the next reboot instead of failing the uninstall.
    RMDir /r /REBOOTOK "$1\DiscordBridge"
    ${If} ${FileExists} "$1\DiscordBridge\*.*"
      DetailPrint "Не удалось полностью удалить $1\DiscordBridge"
    ${EndIf}
  ${EndIf}
  ; User strategies and settings live in %APPDATA%\io.github.stardely62.discordbridge
  ; and are removed only when the user ticks "delete application data".
!macroend
