**Whisper {{VERSION}}**, based on Firefox {{FIREFOX_VERSION}} · a quiet browser built from Firefox's source. *Whisper can't make you invisible. It just won't snitch.*

### Install

| System | Download | Notes |
|---|---|---|
| Windows 10/11 (x64) | [Whisper-Setup.exe](https://github.com/footprintsntrails/whisper-browser/releases/download/{{TAG}}/Whisper-Setup.exe) | Installs for you only, no administrator rights. Not code-signed: SmartScreen shows "Windows protected your PC"; choose **More info → Run anyway**. Also as a [zip](https://github.com/footprintsntrails/whisper-browser/releases/download/{{TAG}}/whisper-windows-x86_64.zip). |
| Linux (x86-64) | [whisper-linux-x86_64.tar.xz](https://github.com/footprintsntrails/whisper-browser/releases/download/{{TAG}}/whisper-linux-x86_64.tar.xz) | `tar -xf whisper-linux-x86_64.tar.xz && ./whisper/install.sh` installs it for you (no root) into `~/.local/share/whisper`, with a menu entry and the `whisper` command. |

### Check your download

Every file is listed in `SHA256SUMS`:

```sh
sha256sum -c SHA256SUMS --ignore-missing
```

### Updates

Whisper updates itself with Firefox's own updater: it checks Whisper's update host every
6 hours while it runs, downloads in the background and asks you to restart. An update is
installed only if it is signed with one of the two keys built into Whisper, is meant for
Whisper's release channel and is newer than the running build. Installs in a folder you
can write to (the Windows installer's default, a Linux folder in your home) update
without administrator rights.

The `.mar` files are the complete updates; `update-*.xml` are the files the update host
serves. Certificates in the updater (SHA-256):

- primary: `{{MAR_PRIMARY_SHA256}}`
- secondary (rotation): `{{MAR_SECONDARY_SHA256}}`
