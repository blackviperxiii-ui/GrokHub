//! Run one script with the OS's own admin prompt in front of it: polkit's
//! password dialog (`pkexec`) on Linux, the UAC prompt on Windows. The user
//! types the password or presses Yes in that prompt themselves; nothing here
//! sees it. Saying no to the prompt is an answer, not an error.

use std::process::Command;

/// How an elevated run went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Elevated {
    /// The script ran as admin and exited 0.
    Done,
    /// The user closed the prompt or said no.
    Dismissed,
    /// No prompt can show here (no `pkexec`, or no polkit agent running, as
    /// on some tiling desktops). The steps have to be run by hand.
    NoPrompt,
    /// The script ran and failed; the last line it printed.
    Failed(String),
}

/// pkexec's exit code when the user dismissed the dialog.
const PKEXEC_DISMISSED: i32 = 126;
/// pkexec's exit code when no agent could ask, or the user isn't allowed.
const PKEXEC_NOT_AUTHORIZED: i32 = 127;
/// ERROR_CANCELLED: the user said no to the UAC prompt.
const UAC_CANCELLED: i32 = 1223;

/// Runs `script` as root with `/bin/sh -e` behind the polkit prompt.
pub fn root_sh(script: &str) -> Elevated {
    let out = Command::new("pkexec")
        .arg("/bin/sh")
        .arg("-e")
        .arg("-c")
        .arg(script)
        .output();
    match out {
        Err(_) => Elevated::NoPrompt,
        Ok(o) => pkexec_outcome(o.status.code(), &String::from_utf8_lossy(&o.stderr)),
    }
}

/// What pkexec's exit code and error text mean.
pub fn pkexec_outcome(code: Option<i32>, stderr: &str) -> Elevated {
    match code {
        Some(0) => Elevated::Done,
        Some(PKEXEC_DISMISSED) => Elevated::Dismissed,
        Some(PKEXEC_NOT_AUTHORIZED) if stderr.contains("authentication agent") => {
            Elevated::NoPrompt
        }
        Some(PKEXEC_NOT_AUTHORIZED) if stderr.contains("Not authorized") => Elevated::Dismissed,
        _ => Elevated::Failed(last_line(stderr)),
    }
}

/// Runs a PowerShell `script` as administrator behind the UAC prompt, and
/// waits for it.
pub fn admin_ps(script: &str) -> Elevated {
    let outer = uac_wrapper(script);
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
        .arg(encode_ps(&outer));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    match cmd.output() {
        Err(_) => Elevated::NoPrompt,
        Ok(o) => match o.status.code() {
            Some(0) => Elevated::Done,
            Some(UAC_CANCELLED) => Elevated::Dismissed,
            _ => Elevated::Failed(last_line(&String::from_utf8_lossy(&o.stderr))),
        },
    }
}

/// The unelevated PowerShell that starts `script` elevated, waits, and
/// passes its exit code on; a no at the UAC prompt exits 1223.
pub fn uac_wrapper(script: &str) -> String {
    format!(
        "try {{ $p = Start-Process powershell.exe -Verb RunAs -Wait -PassThru -WindowStyle Hidden \
         -ArgumentList '-NoProfile','-NonInteractive','-EncodedCommand','{}'; exit $p.ExitCode }} \
         catch {{ if ($_.Exception.NativeErrorCode -eq {UAC_CANCELLED}) {{ exit {UAC_CANCELLED} }}; \
         [Console]::Error.WriteLine($_.Exception.Message); exit 1 }}",
        encode_ps(script)
    )
}

/// PowerShell's `-EncodedCommand` form: base64 of the UTF-16LE text, so no
/// quoting ever reaches a command line.
pub fn encode_ps(script: &str) -> String {
    use base64::Engine as _;
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn last_line(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .unwrap_or("it stopped without saying why");
    line.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkexec_codes_read_as_answers() {
        assert_eq!(pkexec_outcome(Some(0), ""), Elevated::Done);
        assert_eq!(pkexec_outcome(Some(126), ""), Elevated::Dismissed);
        assert_eq!(
            pkexec_outcome(
                Some(127),
                "Error executing command as another user: No authentication agent found.\n"
            ),
            Elevated::NoPrompt
        );
        assert_eq!(
            pkexec_outcome(
                Some(127),
                "Error executing command as another user: Not authorized\n"
            ),
            Elevated::Dismissed
        );
        assert_eq!(
            pkexec_outcome(Some(1), "setcap: command not found\n\n"),
            Elevated::Failed("setcap: command not found".into())
        );
    }

    #[test]
    fn powershell_gets_utf16_base64_and_no_raw_quotes() {
        // "a'" in UTF-16LE is 61 00 27 00.
        assert_eq!(encode_ps("a'"), "YQAnAA==");
        let outer = uac_wrapper("Add-LocalGroupMember -Member 'O''Brien'");
        assert!(outer.contains("-Verb RunAs -Wait -PassThru"));
        assert!(outer.contains("exit 1223"));
        assert!(!outer.contains("O''Brien"), "{outer}");
    }
}
