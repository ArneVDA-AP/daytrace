; Daytrace installer hooks (tauri.conf.json: bundle > windows > nsis > installerHooks).
;
; After installing, the install folder goes on the current user's PATH, so `daytrace-mcp` works
; in any terminal and MCP clients (the Daytrace plugin for Claude Code) can start it by name.
; Uninstalling takes it off again. daytrace-mcp does the work itself (`link` / `unlink`: the Path
; value under HKCU\Environment, then a WM_SETTINGCHANGE broadcast), so a long PATH is never cut
; to the length of an NSIS string. nsExec runs it without a console window.
;
; Before an update installs, Daytrace moves daytrace-mcp.exe aside (daytrace-mcp.exe.<time>.old,
; features/platform_extras/autoupdate.rs) because an MCP client may keep it running, and removes
; those copies when it starts. Uninstalling removes the copies that are left, so the install
; folder can go; a copy that is still running can't be removed and stays.

!macro NSIS_HOOK_POSTINSTALL
  nsExec::Exec '"$INSTDIR\daytrace-mcp.exe" link'
  Pop $0
  DetailPrint "daytrace-mcp link: $0"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; An update runs the old uninstaller with /UPDATE right before installing the new version,
  ; which links the folder again; leave PATH alone then.
  ${If} $UpdateMode <> 1
    nsExec::Exec '"$INSTDIR\daytrace-mcp.exe" unlink'
    Pop $0
    Delete "$INSTDIR\daytrace-mcp*.old"
  ${EndIf}
!macroend
