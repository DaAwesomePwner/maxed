; Tauri 2.12.1 includes installerHooks before declaring its MUI2 pages.
; Configure only the native finish page: never declare an early custom page.
; No dependency detection, download, or browser/installer launch.
Var MaxedNpcapFinishText
!define MUI_FINISHPAGE_TEXT "$MaxedNpcapFinishText"
!define MUI_FINISHPAGE_TEXT_LARGE

!macro NSIS_HOOK_POSTINSTALL
  ; The MUI2 finish page is after INSTFILES and skipped by Tauri in passive mode.
  ; Use its native 60-DLU text area and existing control layout; no custom labels
  ; can overflow the 140-DLU inner dialog used by the previous custom page.
  ; The official address is inline text, with no link or launch handler.
  StrCpy $MaxedNpcapFinishText "Npcap: https://npcap.com/#download. Demo/history work without it. Leave $\"Restrict Npcap driver's access to Administrators only$\" unchecked. Already restricted? Re-run the Npcap installer and untick that option. After installing Npcap, open Maxed and use Retry checks in Setup. Npcap setup may need elevation; elevating Maxed is not a repair. Setup checks compatibility, access and live traffic."
!macroend
