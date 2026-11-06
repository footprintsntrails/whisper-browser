//! The few Windows API calls Whisper needs.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_SETFOREGROUND, MB_YESNO as YESNO,
    MessageBoxW,
};

pub const MB_INFO: u32 = MB_OK | MB_ICONINFORMATION;
pub const MB_ERROR: u32 = MB_OK | MB_ICONERROR;
pub const MB_YESNO: u32 = YESNO | MB_ICONQUESTION;
pub const IDYES: i32 = windows_sys::Win32::UI::WindowsAndMessaging::IDYES;

/// Starts a process without a console window.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Detaches a process from ours so it keeps running after Whisper exits.
pub const DETACHED: u32 = 0x0000_0008 | 0x0000_0200; // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

pub fn message_box(title: &str, text: &str, flags: u32) -> i32 {
    let (title, text) = (wide(title), wide(text));
    // SAFETY: both strings are NUL-terminated UTF-16 buffers that outlive the call.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            flags | MB_SETFOREGROUND,
        )
    }
}

/// Reads `App Paths\<exe>` (the install location most programs register).
pub fn app_path(exe: &str) -> Option<String> {
    let subkey = wide(&format!(
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\{exe}"
    ));
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let mut buf = vec![0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        // SAFETY: `buf` has `size` bytes; a null value name reads the key's default value.
        let status = unsafe {
            RegGetValueW(
                root,
                subkey.as_ptr(),
                std::ptr::null(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if status == 0 {
            let len = (size as usize / 2).saturating_sub(1);
            let value = String::from_utf16_lossy(&buf[..len.min(buf.len())]);
            let value = value.trim_end_matches('\0').trim_matches('"').to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

fn powershell() -> Command {
    let system_root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let exe = Path::new(&system_root).join("System32\\WindowsPowerShell\\v1.0\\powershell.exe");
    let mut cmd = Command::new(exe);
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
    ])
    .creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// Checks that `file` has a valid Authenticode signature whose signer contains `signer`.
pub fn verify_authenticode(file: &Path, signer: &str) -> Result<(), String> {
    let out = powershell()
        .args([
            "-Command",
            "$s = Get-AuthenticodeSignature -LiteralPath $env:WHISPER_CHECK_FILE; \
             [Console]::Out.Write([string]$s.Status + '|' + [string]$s.SignerCertificate.Subject)",
        ])
        .env("WHISPER_CHECK_FILE", file)
        .output()
        .map_err(|e| format!("could not check the signature: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let (status, subject) = text.split_once('|').unwrap_or((&text, ""));
    if status.trim() == "Valid" && subject.contains(signer) {
        Ok(())
    } else {
        Err(format!(
            "the download is not signed by {signer} (status {status}, signer {subject})"
        ))
    }
}

const CHOOSER: &str = r#"
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
[System.Windows.Forms.Application]::EnableVisualStyles()
$f = New-Object System.Windows.Forms.Form
$f.Text = $env:WHISPER_TITLE
$f.StartPosition = 'CenterScreen'
$f.FormBorderStyle = 'FixedDialog'
$f.MaximizeBox = $false
$f.MinimizeBox = $false
$f.TopMost = $true
$f.AutoScaleMode = 'Dpi'
$f.Font = New-Object System.Drawing.Font('Segoe UI', 9)
$f.ClientSize = New-Object System.Drawing.Size(480, 300)
$l = New-Object System.Windows.Forms.Label
$l.Text = $env:WHISPER_PROMPT
$l.SetBounds(14, 12, 452, 44)
$b = New-Object System.Windows.Forms.ListBox
$b.SetBounds(14, 60, 452, 188)
$b.IntegralHeight = $false
foreach ($i in ($env:WHISPER_CHOICES -split "`n")) { [void]$b.Items.Add($i) }
$b.SelectedIndex = 0
$ok = New-Object System.Windows.Forms.Button
$ok.Text = 'Continue'
$ok.SetBounds(296, 260, 82, 28)
$ok.DialogResult = 'OK'
$c = New-Object System.Windows.Forms.Button
$c.Text = 'Cancel'
$c.SetBounds(384, 260, 82, 28)
$c.DialogResult = 'Cancel'
$f.AcceptButton = $ok
$f.CancelButton = $c
$f.Controls.AddRange(@($l, $b, $ok, $c))
$b.Add_DoubleClick({ $f.DialogResult = 'OK'; $f.Close() })
if ($f.ShowDialog() -eq 'OK') { [Console]::Out.Write($b.SelectedIndex) }
"#;

/// A small list dialog (WinForms through Windows PowerShell, which every Windows 10/11 has).
pub fn choose(title: &str, prompt: &str, items: &[String]) -> Option<usize> {
    let script: Vec<u8> = CHOOSER
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    let out = powershell()
        .args(["-EncodedCommand", &base64(&script)])
        .env("WHISPER_TITLE", title)
        .env("WHISPER_PROMPT", prompt)
        .env("WHISPER_CHOICES", items.join("\n"))
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Lets `whisper.exe --version` and friends print to the console they were started from
/// (the program itself has no console window).
pub fn attach_parent_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: no pointers involved; failure just means there is no parent console.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_matches_known_values() {
        assert_eq!(super::base64(b"hello"), "aGVsbG8=");
        assert_eq!(super::base64(b"hi!"), "aGkh");
        assert_eq!(super::base64(b"h"), "aA==");
    }
}
