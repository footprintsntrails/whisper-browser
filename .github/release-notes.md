**Whisper {{VERSION}}** · a quiet browser built on Firefox. *Whisper can't make you invisible. It just won't snitch.*

This is a prototype (stage 0): your installed Firefox with a separate Whisper profile, kept up to date by a small native launcher.

### What's inside

- Telemetry, studies, crash reports, sponsored content and Pocket switched off
- uBlock Origin from addons.mozilla.org, Strict tracking protection, Global Privacy Control
- DNS over HTTPS through Quad9
- The Whisper look: black and platinum, Inter, vertical tabs, the page in its own rounded panel
- Import of bookmarks, history and passwords from Firefox, Zen, LibreWolf, Floorp and Waterfox (copied on your machine), and from Chrome, Edge, Brave, Opera, Vivaldi and Safari through Firefox's import window
- Daily self-update in the background, verified with an RSA signature before anything is installed
- Your normal Firefox profile is never touched

### Install

| System | Download | Notes |
|---|---|---|
| Windows 10/11 | [Whisper-Setup.exe](https://github.com/footprintsntrails/whisper-browser/releases/download/v{{VERSION}}/Whisper-Setup.exe) | Not code-signed yet: SmartScreen shows "Windows protected your PC"; choose **More info → Run anyway**. |
| macOS 11+ (Apple silicon and Intel) | [Whisper.dmg](https://github.com/footprintsntrails/whisper-browser/releases/download/v{{VERSION}}/Whisper.dmg) | Not notarized yet: the first time, right-click Whisper → **Open**, or allow it in System Settings → Privacy & Security. |
| Debian, Ubuntu, Mint (x86-64) | [whisper.deb](https://github.com/footprintsntrails/whisper-browser/releases/download/v{{VERSION}}/whisper.deb) | ARM64: [whisper-arm64.deb](https://github.com/footprintsntrails/whisper-browser/releases/download/v{{VERSION}}/whisper-arm64.deb) |
| Fedora, openSUSE (x86-64) | [whisper.rpm](https://github.com/footprintsntrails/whisper-browser/releases/download/v{{VERSION}}/whisper.rpm) | ARM64: [whisper-aarch64.rpm](https://github.com/footprintsntrails/whisper-browser/releases/download/v{{VERSION}}/whisper-aarch64.rpm) |
| Any Linux (x86-64) | [whisper-linux.tar.gz](https://github.com/footprintsntrails/whisper-browser/releases/download/v{{VERSION}}/whisper-linux.tar.gz) | Unpack, then `./whisper --install` (no root). ARM64: [whisper-linux-arm64.tar.gz](https://github.com/footprintsntrails/whisper-browser/releases/download/v{{VERSION}}/whisper-linux-arm64.tar.gz) |

Whisper needs Firefox. Windows and macOS offer to download it from mozilla.org (and check Mozilla's signature); on Linux, install Firefox from your package manager (native, Snap or Flatpak).

### Check your download

Every file is listed in `SHA256SUMS`:

```sh
sha256sum -c SHA256SUMS --ignore-missing      # Linux
shasum -a 256 -c SHA256SUMS --ignore-missing  # macOS
```

`update.json` lists the same files with their SHA-256 and is signed (`update.json.sig`, RSA-3072, PKCS#1 v1.5, SHA-256) with the key in [`keys/update-signing.pub.pem`](https://github.com/footprintsntrails/whisper-browser/blob/main/keys/update-signing.pub.pem). Installed copies of Whisper only install an update whose signature checks out.
