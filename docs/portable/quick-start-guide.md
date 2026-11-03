# NanoNexus on a USB stick: quick start

This guide puts the complete **DIST** folder on a USB stick so NanoNexus runs from it on a Mac (Apple Silicon) or a Windows PC (64-bit), with your models, chats and files staying on the stick.

**Honest status:** the apps were built but not tested on any machine. Steps marked *(untested)* have never been tried. Linux can prepare the stick but cannot run the app (no Linux build exists).

## 1. What you need

- A USB stick or SSD. 64 GB or more is comfortable; models are the big part (1 to 8 GB each). Faster is better: USB 3 or newer.
- The `DIST` folder you were given (ask for it as a plain folder, not a zip, to avoid copying twice).
- A Mac with Apple Silicon (M1 or newer, macOS 13 or newer) and/or a Windows 10 or 11 PC (64-bit). PCs from about 2017 onward are fine; the engines pick a CPU variant at start.
- Windows needs Microsoft Edge WebView2. Windows 11 and up-to-date Windows 10 already have it.

## 2. Back up, then format the stick as exFAT

**Formatting erases everything on the stick.** Copy anything you want to keep somewhere else first. NanoNexus never formats a drive.

exFAT is the format both macOS and Windows read and write.

### On a Mac

1. Plug in the stick. Open **Disk Utility**.
2. Choose **View, Show All Devices**. Select the stick itself (the top line), not the indented volume.
3. Click **Erase**. Name it `NANONEXUS`. Format: **ExFAT**. Scheme: **Master Boot Record** (best for Windows compatibility).
4. Click **Erase** and wait.

### On Windows

1. Plug in the stick. Open **This PC**, right-click the stick, choose **Format**.
2. File system: **exFAT**. Allocation unit size: **Default**. Volume label: `NANONEXUS`.
3. Leave **Quick Format** on. Click **Start** and confirm that you accept the erase.

### On Linux

```
lsblk                                   # find the stick, for example /dev/sdb. Check it twice.
sudo umount /dev/sdb1
sudo mkfs.exfat -n NANONEXUS /dev/sdb1  # needs the exfatprogs package
```

Replace `/dev/sdb1` with your stick. A wrong device name erases the wrong disk.

## 3. Copy the DIST to the stick

Copy the **whole** folder contents so that `portable.json` sits at the top of the stick (or inside one folder of your choice). Do not skip hidden files.

- Mac: drag the contents of `DIST` onto the stick in Finder, or run `cp -R /path/to/DIST/. /Volumes/NANONEXUS/`.
- Windows: copy the contents of `DIST` in Explorer and paste them into the stick.
- Linux: `cp -r /path/to/DIST/. /media/$USER/NANONEXUS/`.

The folder should now look like this:

```
portable.json
Start-Mac.command
Start-Windows.cmd
apps/macos-arm64/NanoNexus.app
apps/windows-x64/NanoNexus.exe
runtimes/...        engines
models/             empty at first, your shared model store
data/ outputs/ cache/ logs/
licenses/
manifest.json       SHA-256 of every shipped file
quick-start-guide.html
```

Do not move `NanoNexus.app` or `NanoNexus.exe` out of `apps/`. They refuse to start outside the DIST on purpose, so they never write into your computer's profile by mistake.

## 4. Start it

### Mac *(untested)*

1. Open the stick in Finder and double-click **Start-Mac.command**.
2. If macOS says the file or app cannot be opened because the developer cannot be verified: right-click it, choose **Open**, then **Open** again. The app is not notarized by Apple.
3. If double-click does nothing, open Terminal and run `sh /Volumes/NANONEXUS/Start-Mac.command`.

### Windows *(untested)*

1. Open the stick in Explorer and double-click **Start-Windows.cmd**.
2. If SmartScreen says "Windows protected your PC", click **More info**, then **Run anyway**. The app is not code-signed.
3. If a window says WebView2 is missing, install the Microsoft Edge WebView2 Runtime once on that PC.

### Linux

There is no Linux app in this DIST. A Linux machine can prepare the stick (steps 2 and 3) and manage files on it.

## 5. First run: get a model

The stick ships without chat models so that it stays small and shared between Mac and Windows.

1. Connect to the internet for this step only.
2. Open **Models**. In the **Catalog** pick a small model first (about 2B parameters on 8 GB RAM or less), then press **Download**.
3. For images, speech and voice, use **Models, USB Studio**: pick a category, give a Hugging Face file link or import a file you already have.
4. Downloads land in `models/` on the stick. Each file is stored once and shared by the Mac and Windows apps.

After this you can disconnect. Chat, models, images and transcripts all work offline. Only model downloads and the optional web search need the internet.

## 6. Where your things are

| What | Where on the stick |
|---|---|
| Models | `models/` |
| Chats, settings, knowledge, change history | `data/` |
| Images, audio, transcripts | `outputs/` |
| Temporary and cache files | `cache/<platform>/` |
| Logs | `logs/<platform>/` |

Settings, then Storage activity, shows which writes the app made and where, and which destinations it uses. It lists what it cannot see. The computer's operating system can still keep small traces of its own, such as recent-file lists, which no app controls.

## 7. Working on your own files

NanoNexus only touches folders you grant. Grants are re-asked on every launch and on every new computer. Put your projects in a folder next to `DIST` on the stick (for example `Projects/`), not inside `DIST`.

## 8. Update safely

1. Close NanoNexus on every computer. Eject nothing yet.
2. On a computer with Node 22 or newer, in the project folder, run:

```
node scripts/portable-assemble.mjs --spec dist-spec.json --out /Volumes/NANONEXUS --update
```

3. The old app and engine versions are kept in `.rollback/` on the stick. `models/`, `data/`, `outputs/`, `cache/` and `logs/` are never touched. Do not run an older app against data written by a newer one.

## 9. Eject safely

Close NanoNexus first (quit it from its window, not just by closing the lid). Then eject: Finder eject button on Mac; **Safely Remove Hardware** on Windows; `umount` on Linux. Pulling the stick while the app runs can corrupt chats or half-finished downloads.

## 10. If something goes wrong

- **"This app requires the complete USB DIST":** the app was moved or the stick copy is incomplete. Copy the whole DIST again.
- **"Not enough free space":** the app needs at least 64 MB free for its state, and much more for models. Free space on the stick.
- **Stick is read-only:** check the lock switch on the stick; on Mac do not mount it with "Ignore ownership" problems; on Windows run the format check.
- **Slow replies on an old PC:** use a smaller model. Settings, This computer, shows memory-based advice.
- **Engine missing:** Studio tabs say which engine is not in this DIST. See `licenses/` and `manifest.json` for what was shipped.

## 11. What was and was not verified

Not tested: starting on any Mac or Windows PC, running from exFAT, Mac Gatekeeper behaviour, Windows SmartScreen behaviour, older CPUs, image, speech or voice runs, and updates. Voice (Kokoro) is not in this DIST. Speech on the Mac is not in this DIST unless `runtimes/macos-arm64/speech/` exists. Windows 10 PCs without WebView2 need it installed once.
