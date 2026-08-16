pub struct DoctorLine {
    pub ok: bool,
    pub text: String,
}

pub fn doctor_lines(has_key: bool, memory_dir_ok: bool, hub_kind: &str) -> Vec<DoctorLine> {
    vec![
        DoctorLine {
            ok: has_key,
            text: if has_key {
                "xAI auth present".into()
            } else {
                "xAI auth missing — Connect Grok OAuth in Settings".into()
            },
        },
        DoctorLine {
            ok: memory_dir_ok,
            text: if memory_dir_ok {
                "memory dir readable".into()
            } else {
                "memory dir missing".into()
            },
        },
        DoctorLine {
            ok: hub_kind == crate::HUB_KIND,
            text: format!("hub kind {hub_kind}"),
        },
    ]
}

pub fn doctor_ok(lines: &[DoctorLine]) -> bool {
    lines.iter().all(|l| l.ok)
}

pub fn doctor_extras(last_receipt_ok: Option<bool>, skill_count: usize) -> Vec<DoctorLine> {
    let mut out = Vec::new();
    match last_receipt_ok {
        Some(true) => out.push(DoctorLine {
            ok: true,
            text: "last host receipt ok".into(),
        }),
        Some(false) => out.push(DoctorLine {
            ok: false,
            text: "last host receipt failed".into(),
        }),
        None => out.push(DoctorLine {
            ok: true,
            text: "no host receipt yet".into(),
        }),
    }
    out.push(DoctorLine {
        ok: true,
        text: format!("{skill_count} skills on disk"),
    });
    out
}

pub fn doctor_hands_line(driver: &str) -> DoctorLine {
    let ok = driver != "missing"
        && driver != "not installed"
        && driver != "uinput"
        && driver != "daemon";
    DoctorLine {
        ok,
        text: format!("hands {driver}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HUB_KIND;

    #[test]
    fn doctor_fails_without_key() {
        let lines = doctor_lines(false, true, HUB_KIND);
        assert!(!doctor_ok(&lines));
        assert!(lines[0].text.contains("Connect Grok"));
    }

    #[test]
    fn doctor_ok_when_ready() {
        let lines = doctor_lines(true, true, HUB_KIND);
        assert!(doctor_ok(&lines));
        let extra = doctor_extras(Some(false), 3);
        assert!(!doctor_ok(&extra));
        assert!(extra[1].text.contains("3 skills"));
        let hands = doctor_hands_line("ydotool");
        assert!(hands.ok);
        assert!(hands.text.contains("ydotool"));
        assert!(!doctor_hands_line("missing").ok);
        assert!(!doctor_hands_line("not installed").ok);
        assert!(!doctor_hands_line("uinput").ok);
        assert!(!doctor_hands_line("daemon").ok);
    }
}
