# Windows testing

Run this checklist on every Windows build you hand to someone (M13). Two machines cover it:

| Machine | What it proves |
|---|---|
| **Windows 11 ARM VM** on the Mac (UTM or Parallels) | The arm64 build runs natively; the x64 build runs under Prism x64 emulation (CPU only, slower, fine for smoke tests). |
| **Real x64 PC** (the studio PC), **offline** | Install without internet, Vulkan GPU, performance, the offline kit. |

Record results in `docs/SPIKES.md` (S7 cross-compile, S8 Vulkan) and tick `T13.11` / `T13.13` in `tasks.md`.
Mark each line ✅ / ❌ with the build name, the machine and the date.

## 0. Before you start
- [ ] Build on the Mac: `scripts/build-windows.sh --profile <p> --arch x64 --format installer` (and `--arch arm64`, `--format portable`, `--format kit`). The log ends with `✔` and `dist/<p>/SHA256SUMS-windows-<arch>.txt` exists.
- [ ] Copy the files to the test machine (USB or shared folder). Compare `Get-FileHash <file> -Algorithm SHA256` with `SHA256SUMS-windows-<arch>.txt`.
- [ ] **Turn Wi-Fi/Ethernet off** on the x64 PC before the offline runs. (Settings → Network → disconnect.)
- [ ] Windows 11 is the target. For a Windows 10 spot check use a machine without the WebView2 runtime only with the **kit** (it embeds the offline installer).

## 1. Installer (`…-setup.exe`)
- [ ] Double-click. Windows SmartScreen says "Windows protected your PC" (the build is unsigned): **More info → Run anyway**.
- [ ] No admin prompt (per-user install into `%LOCALAPPDATA%\Programs\NanoNexus`).
- [ ] The app starts from the Start menu. The title bar is the normal Windows one with the Mica backdrop (Windows 11), no blank white flash.
- [ ] The runtime panel goes Starting → **Ready** within ~10 s with the bundled model. No console window flashes at any point.
- [ ] Ask "Say hello in five words." The answer streams. Stop works while streaming.
- [ ] Models page: the bundled model(s) show as **Bundled** and cannot be deleted. **Verify bundled** passes.
- [ ] Settings → About shows the profile, commit and build date.
- [ ] Shortcuts use **Ctrl** (Ctrl+K palette, Ctrl+N, Ctrl+,) and the hints say "Ctrl", not "⌘".
- [ ] Uninstall from Settings → Apps. The install folder is removed; `%APPDATA%\dev.nanonexus.app` (your chats) stays.

## 2. No orphan process (Job Object)
- [ ] With the app running and the model Ready, open Task Manager → Details. `nano-nexus.exe` and `llama-server.exe` are listed.
- [ ] **End task** on `nano-nexus.exe` (not a clean quit). Within a second `llama-server.exe` disappears too.
- [ ] Quit normally from the app. Both are gone.
- [ ] Start the app again: Ready again (no "port in use" error).

## 3. Portable ZIP
- [ ] Unzip to `D:\Test\NanoNexus-portable` (a path with a space is better: `D:\My Tests\…`). Run `nano-nexus.exe`.
- [ ] A `data\` folder appears next to the exe (settings, chats, logs). Nothing is written to `%APPDATA%\dev.nanonexus.app`.
- [ ] Copy the whole folder to another place: the chats are still there.

## 4. Offline kit (x64 PC, **no internet**)
- [ ] The kit folder has `…-setup.exe`, `models-extra\pack.json`, `models-extra\models\…`, `INSTALL.txt`.
- [ ] Run the installer from that folder. After it finishes, `%LOCALAPPDATA%\Programs\NanoNexus\models-extra\pack.json` exists.
- [ ] First launch: the models from `models-extra` appear under **Bundled** within about a minute (they are checked by SHA-256 first). Switch between all models; each answers.
- [ ] Damage test: copy the kit, flip one byte of one `models-extra\models\…\*.gguf`, install. That model is **not** listed and a notification says which one failed; the others work.
- [ ] A second launch is fast (nothing is hashed again).

## 5. Model packs
- [ ] Models → **Export model pack…** to a folder, then **Import model pack…** on another install (or after deleting a downloaded model). Progress shows; the model returns, verified.
- [ ] Import with "Copy" off keeps working while the folder is connected and is listed as imported.

## 6. Files and folders
- [ ] Give a folder under Documents read access. Ask an assistant to list it. Works with long paths and spaces.
- [ ] A Rename Forge / Tidy change set applies and **Undo** restores the files byte for byte. Deleted files go to the Recycle Bin.
- [ ] **Controlled folder access** (Windows Security → Ransomware protection) on: writing into Documents fails with a clear message that names Windows Security, not a generic error. Allow the app, retry, it works.
- [ ] Paths such as `C:\Windows\System32` and `%APPDATA%\Microsoft\Credentials` are refused even if a parent folder was granted.

## 7. Vulkan and CPU fallback (x64 PC with a GPU) — spike S8
- [ ] The runtime panel says **Vulkan** (log line `using device Vulkan0`). Note tokens/s for the default model: ___ (GPU) .
- [ ] Rename `C:\Windows\System32\vulkan-1.dll` is **not** allowed. Instead run on a VM or PC without a GPU driver: the runtime panel says **CPU** and chat still works.
- [ ] Record both numbers and the machine in `docs/SPIKES.md` §S8.

## 8. LAN Share and the Windows firewall
The first time sharing starts, Windows Defender Firewall asks whether to allow NanoNexus on **Private networks**
or **Public networks**. Tick **Private networks only**. If the prompt was dismissed:

1. Windows Security > Firewall & network protection > Allow an app through firewall.
2. Find NanoNexus and tick **Private**; leave **Public** unticked.
3. From a phone on the same Wi-Fi open `http://<pc-ip>:9889`. `ipconfig` shows the IPv4 address.

Checks: `netstat -ano | findstr 9889` shows the app listening, and turning sharing off closes the port.
Only the share port is reachable from the network; `llama-server` stays on 127.0.0.1.

## 9. Windows 11 ARM VM extras
- [ ] The arm64 installer or portable ZIP starts and reaches Ready (CPU).
- [ ] The **x64** portable ZIP also starts under emulation and reaches Ready (slow is fine).
- [ ] `llama-server.exe` in the arm64 build is arm64: Task Manager → Details → Architecture column shows **ARM64**.

## When something fails
Collect: the app's **Settings → Diagnostics** report, `data\logs\llama-server.log` (portable) or
`%LOCALAPPDATA%\dev.nanonexus.app\logs\`, the Windows version (`winver`), and the build name. Add a line to
`docs/SPIKES.md` and, if it needs a decision, a question to `docs/QUESTIONS.md`.
