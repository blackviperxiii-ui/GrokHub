#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenSize {
    pub w: i32,
    pub h: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComputerOp {
    Click { x: i32, y: i32 },
    Act { name: String },
    WaitFor { title: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipe {
    pub screen: Option<ScreenSize>,
    pub ops: Vec<ComputerOp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayOp {
    Reshoot,
    Op(ComputerOp),
}

pub fn parse_screen(s: &str) -> Option<ScreenSize> {
    let s = s.trim().strip_prefix("screen=").unwrap_or(s.trim());
    let (w, h) = s.split_once('x')?;
    Some(ScreenSize {
        w: w.trim().parse().ok()?,
        h: h.trim().parse().ok()?,
    })
}

pub fn needs_reshoot(recipe: Option<ScreenSize>, current: Option<ScreenSize>) -> bool {
    match (recipe, current) {
        (Some(r), Some(c)) => r != c,
        (Some(_), None) => true,
        _ => false,
    }
}

pub fn parse_computer_op(line: &str) -> Option<ComputerOp> {
    let rest = line
        .trim()
        .strip_prefix("COMPUTER_CMD:")
        .or_else(|| line.trim().strip_prefix("COMPUTER_CMD"))?;
    let rest = rest.trim().trim_start_matches(':').trim();
    if rest.is_empty() {
        return None;
    }
    let mut bits = rest.split_whitespace();
    let op = bits.next()?.to_ascii_lowercase();
    match op.as_str() {
        "click" => {
            let x = bits.next()?.parse().ok()?;
            let y = bits.next()?.parse().ok()?;
            Some(ComputerOp::Click { x, y })
        }
        "act" => {
            let name = bits.collect::<Vec<_>>().join(" ");
            if name.is_empty() {
                None
            } else {
                Some(ComputerOp::Act { name })
            }
        }
        "wait_for" | "wait-for" => {
            let rest = bits.collect::<Vec<_>>().join(" ");
            let title = rest
                .strip_prefix("title=")
                .or_else(|| rest.strip_prefix("title:"))
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty());
            Some(ComputerOp::WaitFor { title })
        }
        _ => None,
    }
}

pub fn parse_recipe(text: &str) -> Option<Recipe> {
    let mut screen = None;
    let mut ops = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("RECIPE:") {
            screen = parse_screen(rest);
            continue;
        }
        if let Some(op) = parse_computer_op(t) {
            ops.push(op);
        }
    }
    if screen.is_none() && ops.is_empty() {
        None
    } else {
        Some(Recipe { screen, ops })
    }
}

pub fn replay_ops(recipe: &Recipe, current: Option<ScreenSize>) -> Vec<ReplayOp> {
    let reshoot = needs_reshoot(recipe.screen, current);
    let mut out = Vec::new();
    if reshoot {
        out.push(ReplayOp::Reshoot);
    }
    for op in &recipe.ops {
        match op {
            ComputerOp::Click { .. } if reshoot => {}
            other => out.push(ReplayOp::Op(other.clone())),
        }
    }
    out
}

pub fn screen_from_extents(max_x: i32, max_y: i32) -> Option<ScreenSize> {
    if max_x > 0 && max_y > 0 {
        Some(ScreenSize { w: max_x, h: max_y })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reshoot_skips_coordinates() {
        assert_eq!(
            parse_computer_op("COMPUTER_CMD: act Refresh"),
            Some(ComputerOp::Act {
                name: "Refresh".into()
            })
        );
        assert_eq!(
            parse_computer_op("COMPUTER_CMD: wait_for title=Settings"),
            Some(ComputerOp::WaitFor {
                title: Some("Settings".into())
            })
        );
        let recipe = parse_recipe(
            "RECIPE: screen=1920x1080\nCOMPUTER_CMD: click 10 20\nCOMPUTER_CMD: act Refresh\n",
        )
        .unwrap();
        assert_eq!(recipe.screen, Some(ScreenSize { w: 1920, h: 1080 }));
        assert!(needs_reshoot(recipe.screen, Some(ScreenSize { w: 1280, h: 720 })));
        assert!(!needs_reshoot(recipe.screen, Some(ScreenSize { w: 1920, h: 1080 })));
        let replay = replay_ops(&recipe, Some(ScreenSize { w: 800, h: 600 }));
        assert_eq!(replay[0], ReplayOp::Reshoot);
        assert!(!replay.iter().any(|r| matches!(r, ReplayOp::Op(ComputerOp::Click { .. }))));
        assert!(replay
            .iter()
            .any(|r| matches!(r, ReplayOp::Op(ComputerOp::Act { name }) if name == "Refresh")));
    }
}
