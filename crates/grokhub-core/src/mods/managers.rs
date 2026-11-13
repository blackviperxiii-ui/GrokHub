//! Mod managers that may already look after a game. Read-only: Loadout
//! looks for their marks and, when one manages the game, leaves installs
//! to it instead of writing beside it.

use std::path::Path;

/// The folders the checks read. `None` skips the checks that need it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Places<'a> {
    /// The game's install folder.
    pub game_dir: Option<&'a Path>,
    /// Roaming app data (`%APPDATA%`, `~/.config` elsewhere).
    pub appdata: Option<&'a Path>,
    /// Local app data (`%LOCALAPPDATA%`).
    pub local_appdata: Option<&'a Path>,
}

/// The name r2modman and the Thunderstore app keep a game's profiles under.
fn thunderstore_name(game_id: &str) -> Option<&'static str> {
    Some(match game_id {
        "valheim" => "Valheim",
        "lethalcompany" => "LethalCompany",
        "ror2" => "RiskOfRain2",
        "repo" => "REPO",
        _ => return None,
    })
}

fn norm(p: &str) -> String {
    p.replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

/// Whether one Mod Organizer 2 instance points at this game folder.
fn mo2_points_at(ini: &str, game_dir: &Path) -> bool {
    let want = norm(&game_dir.to_string_lossy());
    ini.lines().any(|l| {
        let Some(v) = l.trim().strip_prefix("gamePath=") else {
            return false;
        };
        let v = v
            .strip_prefix("@ByteArray(")
            .and_then(|v| v.strip_suffix(')'))
            .unwrap_or(v);
        norm(&v.replace("\\\\", "\\")) == want
    })
}

/// The mod manager looking after this game, if Loadout can see one.
pub fn managing(game_id: &str, at: Places<'_>) -> Option<&'static str> {
    if let Some(dir) = at.game_dir {
        if dir.join("vortex.deployment.json").is_file()
            || dir.join("Data").join("vortex.deployment.json").is_file()
        {
            return Some("Vortex");
        }
        if let Some(local) = at.local_appdata {
            let instances = std::fs::read_dir(local.join("ModOrganizer"));
            for e in instances.into_iter().flatten().flatten() {
                let ini =
                    std::fs::read_to_string(e.path().join("ModOrganizer.ini")).unwrap_or_default();
                if mo2_points_at(&ini, dir) {
                    return Some("Mod Organizer 2");
                }
            }
        }
    }
    if let (Some(name), Some(appdata)) = (thunderstore_name(game_id), at.appdata) {
        for (app, base) in [
            ("r2modman", appdata.join("r2modmanPlus-local")),
            (
                "Thunderstore Mod Manager",
                appdata.join("Thunderstore Mod Manager").join("DataFolder"),
            ),
        ] {
            let profiles = base.join(name).join("profiles");
            if std::fs::read_dir(&profiles).is_ok_and(|mut rd| rd.next().is_some()) {
                return Some(app);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::testing::temp_config;

    #[test]
    fn each_managers_mark_is_found() {
        let base = temp_config("managers");
        let game = base.join("Skyrim Special Edition");
        let appdata = base.join("Roaming");
        let local = base.join("Local");
        std::fs::create_dir_all(game.join("Data")).unwrap();
        let at = Places {
            game_dir: Some(&game),
            appdata: Some(&appdata),
            local_appdata: Some(&local),
        };
        assert_eq!(managing("skyrimse", at), None);

        std::fs::create_dir_all(local.join("ModOrganizer/Skyrim")).unwrap();
        let ini = format!(
            "[General]\ngameName=Skyrim Special Edition\ngamePath=@ByteArray({})\n",
            game.to_string_lossy().replace('/', "\\\\")
        );
        std::fs::write(local.join("ModOrganizer/Skyrim/ModOrganizer.ini"), ini).unwrap();
        assert_eq!(managing("skyrimse", at), Some("Mod Organizer 2"));

        std::fs::write(game.join("Data/vortex.deployment.json"), "{}").unwrap();
        assert_eq!(managing("skyrimse", at), Some("Vortex"));

        assert_eq!(
            managing(
                "valheim",
                Places {
                    appdata: Some(&appdata),
                    ..Default::default()
                }
            ),
            None
        );
        std::fs::create_dir_all(appdata.join("r2modmanPlus-local/Valheim/profiles/Default"))
            .unwrap();
        assert_eq!(
            managing(
                "valheim",
                Places {
                    appdata: Some(&appdata),
                    ..Default::default()
                }
            ),
            Some("r2modman")
        );
        assert!(!mo2_points_at("gamePath=C:\\\\Other", &game));
    }
}
