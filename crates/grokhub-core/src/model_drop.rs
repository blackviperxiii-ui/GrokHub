//! A new 3D model in Downloads → offer the next step. Opt-in. The cabin lists
//! Downloads and keeps only names with a model extension; nothing else is
//! opened or read. For a new model it renders a small preview here (STL, OBJ,
//! or a 3MF's own thumbnail or mesh), finds the installed slicers, and offers
//! one-taps. Starting a print goes through the agent, so it asks first.

use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use crate::chips::{ChipKind, QuickChip};

pub const EXTENSIONS: &[&str] = &["stl", "3mf", "obj", "step", "stp"];
/// Larger models get no preview; the card still offers the one-taps.
pub const PREVIEW_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// Triangles drawn for a preview; bigger meshes are sampled.
const PREVIEW_TRIS: usize = 200_000;

/// Whether a file name has a model extension. The only test a file gets
/// before the cabin looks at it any further.
pub fn is_model(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// The user's Downloads folder: `XDG_DOWNLOAD_DIR` from `user-dirs.dirs`
/// when set, else `~/Downloads`.
pub fn downloads_dir(home: &Path, user_dirs: Option<&str>) -> PathBuf {
    let xdg = user_dirs.and_then(|text| {
        text.lines().find_map(|line| {
            let value = line.trim().strip_prefix("XDG_DOWNLOAD_DIR=")?;
            let value = value.trim().trim_matches('"');
            let value = value.replace("$HOME", &home.to_string_lossy());
            (!value.is_empty()).then(|| PathBuf::from(value))
        })
    });
    xdg.unwrap_or_else(|| home.join("Downloads"))
}

/// A model file found in Downloads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelFile {
    pub path: PathBuf,
    pub name: String,
    pub bytes: u64,
}

/// The models in `dir` not in `seen`. The first scan (empty `seen`, `first`
/// true) only records what is already there, so old downloads never offer.
pub fn scan_new(dir: &Path, seen: &mut BTreeSet<String>, first: bool) -> Vec<ModelFile> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !is_model(&name) || !seen.insert(name.clone()) || first {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_file() {
            out.push(ModelFile {
                path: entry.path(),
                name,
                bytes: meta.len(),
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

type Tri = [[f32; 3]; 3];

/// A model's triangles, in its own units (millimetres for STL and 3MF).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub tris: Vec<Tri>,
}

impl Mesh {
    /// Width, depth and height of the bounding box.
    pub fn size(&self) -> Option<[f32; 3]> {
        let (lo, hi) = self.bounds()?;
        Some([hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]])
    }

    fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        let mut pts = self.tris.iter().flatten();
        let first = *pts.next()?;
        let (mut lo, mut hi) = (first, first);
        for p in pts {
            for i in 0..3 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
        Some((lo, hi))
    }
}

/// Binary or ASCII STL.
pub fn parse_stl(bytes: &[u8]) -> Option<Mesh> {
    if bytes.len() >= 84 {
        let count = u32::from_le_bytes(bytes[80..84].try_into().ok()?) as usize;
        if bytes.len() == 84 + count * 50 {
            let f = |at: usize| {
                f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
            };
            let tris = (0..count)
                .map(|i| {
                    let at = 84 + i * 50 + 12;
                    let v = |k: usize| [f(at + k * 12), f(at + k * 12 + 4), f(at + k * 12 + 8)];
                    [v(0), v(1), v(2)]
                })
                .collect();
            return Some(Mesh { tris });
        }
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let verts: Vec<[f32; 3]> = text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("vertex "))
        .filter_map(three)
        .collect();
    let tris: Vec<Tri> = verts.as_chunks::<3>().0.to_vec();
    (!tris.is_empty()).then_some(Mesh { tris })
}

fn three(s: &str) -> Option<[f32; 3]> {
    let mut it = s.split_whitespace().map(|n| n.parse::<f32>().ok());
    Some([it.next()??, it.next()??, it.next()??])
}

/// Wavefront OBJ: `v` and `f` lines, polygons fanned into triangles.
pub fn parse_obj(text: &str) -> Option<Mesh> {
    let mut verts: Vec<[f32; 3]> = Vec::new();
    let mut tris = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("v ") {
            verts.extend(three(rest));
        } else if let Some(rest) = line.strip_prefix("f ") {
            let idx: Vec<usize> = rest
                .split_whitespace()
                .filter_map(|t| {
                    let i: i64 = t.split('/').next()?.parse().ok()?;
                    let i = if i < 0 { verts.len() as i64 + i } else { i - 1 };
                    usize::try_from(i).ok().filter(|i| *i < verts.len())
                })
                .collect();
            for k in 1..idx.len().saturating_sub(1) {
                tris.push([verts[idx[0]], verts[idx[k]], verts[idx[k + 1]]]);
            }
        }
    }
    (!tris.is_empty()).then_some(Mesh { tris })
}

/// A 3MF's own thumbnail (PNG bytes) when it has one, and its mesh.
pub fn parse_3mf(bytes: &[u8]) -> (Option<Vec<u8>>, Option<Mesh>) {
    let Ok(mut zip) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return (None, None);
    };
    let mut read = |name: &str| -> Option<Vec<u8>> {
        let mut file = zip.by_name(name).ok()?;
        let mut out = Vec::new();
        file.read_to_end(&mut out).ok()?;
        Some(out)
    };
    let thumb = read("Metadata/thumbnail.png");
    let mesh = read("3D/3dmodel.model")
        .and_then(|xml| String::from_utf8(xml).ok())
        .and_then(|xml| parse_3mf_model(&xml));
    (thumb, mesh)
}

fn attr(tag: &str, name: &str) -> Option<f32> {
    let key = format!(" {name}=\"");
    let at = tag.find(&key)? + key.len();
    tag[at..].split('"').next()?.parse().ok()
}

/// Vertices and triangles from a 3MF model part (first object's mesh and on).
fn parse_3mf_model(xml: &str) -> Option<Mesh> {
    let mut verts: Vec<[f32; 3]> = Vec::new();
    let mut tris = Vec::new();
    for tag in xml.split('<').skip(1) {
        if tag.starts_with("vertex ") {
            verts.push([attr(tag, "x")?, attr(tag, "y")?, attr(tag, "z")?]);
        } else if tag.starts_with("triangle ") {
            let v = |n: &str| {
                attr(tag, n)
                    .map(|f| f as usize)
                    .filter(|i| *i < verts.len())
            };
            if let (Some(a), Some(b), Some(c)) = (v("v1"), v("v2"), v("v3")) {
                tris.push([verts[a], verts[b], verts[c]]);
            }
        } else if tag.starts_with("/mesh") {
            verts.clear();
        }
    }
    (!tris.is_empty()).then_some(Mesh { tris })
}

/// The mesh for a model file's bytes, by extension. STEP has none here.
pub fn mesh_for(name: &str, bytes: &[u8]) -> (Option<Vec<u8>>, Option<Mesh>) {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "stl" => (None, parse_stl(bytes)),
        "obj" => (None, std::str::from_utf8(bytes).ok().and_then(parse_obj)),
        "3mf" => parse_3mf(bytes),
        _ => (None, None),
    }
}

/// A `size`×`size` RGBA preview: the mesh seen from the front-right and
/// above, flat-shaded on a clear background.
pub fn render(mesh: &Mesh, size: usize) -> Option<Vec<u8>> {
    let (lo, hi) = mesh.bounds()?;
    let center = [0, 1, 2].map(|i| (lo[i] + hi[i]) / 2.0);
    let (yaw, pitch) = (-35f32.to_radians(), 30f32.to_radians());
    let (sy, cy, sp, cp) = (yaw.sin(), yaw.cos(), pitch.sin(), pitch.cos());
    // Z-up model space to screen: x right, y down, depth toward the viewer.
    let view = |p: [f32; 3]| {
        let (x, y, z) = (p[0] - center[0], p[1] - center[1], p[2] - center[2]);
        let x1 = x * cy - y * sy;
        let y1 = x * sy + y * cy;
        [x1, -(z * cp - y1 * sp), z * sp + y1 * cp]
    };
    let step = mesh.tris.len().div_ceil(PREVIEW_TRIS).max(1);
    let tris: Vec<[[f32; 3]; 3]> = mesh
        .tris
        .iter()
        .step_by(step)
        .map(|t| [view(t[0]), view(t[1]), view(t[2])])
        .collect();
    let extent = tris
        .iter()
        .flatten()
        .fold(0f32, |m, p| m.max(p[0].abs()).max(p[1].abs()));
    if extent <= 0.0 || !extent.is_finite() {
        return None;
    }
    let scale = (size as f32 * 0.45) / extent;
    let half = size as f32 / 2.0;
    let mut depth = vec![f32::NEG_INFINITY; size * size];
    let mut rgba = vec![0u8; size * size * 4];
    let light = normalize([-0.4, -0.6, 0.7]);
    for t in &tris {
        let s = t.map(|p| [p[0] * scale + half, p[1] * scale + half, p[2]]);
        let n = normalize(cross(sub(t[1], t[0]), sub(t[2], t[0])));
        let shade = 0.35 + 0.65 * dot(n, light).abs();
        let min_x = s.iter().map(|p| p[0]).fold(f32::MAX, f32::min).max(0.0) as usize;
        let max_x = (s
            .iter()
            .map(|p| p[0])
            .fold(f32::MIN, f32::max)
            .min(size as f32 - 1.0))
        .max(0.0) as usize;
        let min_y = s.iter().map(|p| p[1]).fold(f32::MAX, f32::min).max(0.0) as usize;
        let max_y = (s
            .iter()
            .map(|p| p[1])
            .fold(f32::MIN, f32::max)
            .min(size as f32 - 1.0))
        .max(0.0) as usize;
        let area = edge(s[0], s[1], s[2]);
        if area.abs() < f32::EPSILON {
            continue;
        }
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let p = [x as f32 + 0.5, y as f32 + 0.5, 0.0];
                let (w0, w1, w2) = (
                    edge(s[1], s[2], p) / area,
                    edge(s[2], s[0], p) / area,
                    edge(s[0], s[1], p) / area,
                );
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * s[0][2] + w1 * s[1][2] + w2 * s[2][2];
                let at = y * size + x;
                if z > depth[at] {
                    depth[at] = z;
                    let c = (shade * 255.0) as u8;
                    rgba[at * 4..at * 4 + 4].copy_from_slice(&[
                        c,
                        (c as f32 * 0.85) as u8,
                        (c as f32 * 0.7) as u8,
                        255,
                    ]);
                }
            }
        }
    }
    Some(rgba)
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn normalize(a: [f32; 3]) -> [f32; 3] {
    let len = dot(a, a).sqrt();
    if len > 0.0 {
        a.map(|v| v / len)
    } else {
        a
    }
}
fn edge(a: [f32; 3], b: [f32; 3], p: [f32; 3]) -> f32 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

/// An installed slicer and how to open a file in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slicer {
    pub name: &'static str,
    /// The program, or `flatpak` with the app id first in `args`.
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// Linux: (display name, PATH names, Flatpak app id).
const LINUX_SLICERS: &[(&str, &[&str], &str)] = &[
    (
        "OrcaSlicer",
        &["orca-slicer", "OrcaSlicer"],
        "io.github.softfever.OrcaSlicer",
    ),
    (
        "PrusaSlicer",
        &["prusa-slicer", "PrusaSlicer"],
        "com.prusa3d.PrusaSlicer",
    ),
    ("Creality Print", &["CrealityPrint", "creality-print"], ""),
    (
        "Bambu Studio",
        &["bambu-studio"],
        "com.bambulab.BambuStudio",
    ),
    ("Cura", &["cura", "UltiMaker-Cura"], "com.ultimaker.cura"),
];

/// Windows: (display name, folder under Program Files or its prefix, exe).
const WINDOWS_SLICERS: &[(&str, &str, &str)] = &[
    ("OrcaSlicer", "OrcaSlicer", "orca-slicer.exe"),
    ("PrusaSlicer", "Prusa3D\\PrusaSlicer", "prusa-slicer.exe"),
    (
        "Creality Print",
        "Creality\\Creality Print",
        "CrealityPrint.exe",
    ),
    ("Bambu Studio", "Bambu Studio", "bambu-studio.exe"),
    ("Cura", "UltiMaker Cura", "UltiMaker-Cura.exe"),
];

/// Slicers on Linux: on PATH (`on_path`) or installed as a Flatpak under
/// `flatpak_roots` (`~/.local/share/flatpak/app`, `/var/lib/flatpak/app`).
pub fn linux_slicers(
    on_path: &dyn Fn(&str) -> Option<PathBuf>,
    flatpak_roots: &[PathBuf],
) -> Vec<Slicer> {
    let mut out = Vec::new();
    for (name, bins, flatpak) in LINUX_SLICERS {
        if let Some(program) = bins.iter().find_map(|b| on_path(b)) {
            out.push(Slicer {
                name,
                program,
                args: Vec::new(),
            });
        } else if !flatpak.is_empty() && flatpak_roots.iter().any(|r| r.join(flatpak).is_dir()) {
            out.push(Slicer {
                name,
                program: PathBuf::from("flatpak"),
                args: vec!["run".into(), (*flatpak).into()],
            });
        }
    }
    out
}

/// Slicers on Windows under each Program Files root. Versioned folders
/// ("Creality Print 6.0", "UltiMaker Cura 5.8.1") match by prefix.
pub fn windows_slicers(program_files: &[PathBuf]) -> Vec<Slicer> {
    let mut out = Vec::new();
    for (name, folder, exe) in WINDOWS_SLICERS {
        let (parent, leaf) = match folder.rsplit_once('\\') {
            Some((p, l)) => (Some(p), l),
            None => (None, *folder),
        };
        let found = program_files.iter().find_map(|root| {
            let dir = parent.map_or_else(|| root.clone(), |p| root.join(p));
            let mut dirs: Vec<PathBuf> = std::fs::read_dir(&dir)
                .ok()?
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with(leaf))
                .map(|e| e.path())
                .collect();
            dirs.sort();
            dirs.into_iter()
                .rev()
                .map(|d| d.join(exe))
                .find(|p| p.is_file())
        });
        if let Some(program) = found {
            out.push(Slicer {
                name,
                program,
                args: Vec::new(),
            });
        }
    }
    out
}

fn quote(path: &Path) -> String {
    format!("\"{}\"", path.to_string_lossy())
}

impl Slicer {
    /// The shell line that opens `model` in this slicer.
    pub fn open_line(&self, model: &Path) -> String {
        let mut parts = vec![quote(&self.program)];
        if self.program == Path::new("flatpak") {
            parts[0] = "flatpak".into();
        }
        parts.extend(self.args.iter().cloned());
        parts.push(quote(model));
        parts.join(" ")
    }
}

fn human_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", bytes.div_ceil(1024))
    }
}

/// "60 × 31 × 48 mm".
pub fn dims_line(size: [f32; 3]) -> String {
    format!("{:.0} × {:.0} × {:.0} mm", size[0], size[1], size[2])
}

/// One new model, ready for the Feed and the composer.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelOffer {
    pub file: ModelFile,
    pub dims: Option<[f32; 3]>,
    /// The preview image's key, when one was rendered.
    pub thumb: Option<String>,
}

impl ModelOffer {
    pub fn stem(&self) -> String {
        Path::new(&self.file.name)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| self.file.name.clone())
    }

    pub fn detail(&self) -> String {
        match self.dims {
            Some(d) => format!("{}, {}", human_size(self.file.bytes), dims_line(d)),
            None => human_size(self.file.bytes),
        }
    }

    /// The Feed post: name, size, preview and why it is here.
    pub fn card(&self, slicer: Option<&Slicer>, now_ms: u64) -> crate::UpdateCard {
        let next = match slicer {
            Some(s) => format!(
                "Open it in {}, get a print profile, or move it to your models folder.",
                s.name
            ),
            None => "Get a print profile, or move it to your models folder.".to_string(),
        };
        let mut card = crate::update_feed::digest_card(
            &format!("model-drop:{}", self.file.name),
            &format!("New model: {}", self.file.name),
            &format!("{}. {next}", self.detail()),
            now_ms,
        );
        card.pulse.image_urls = self.thumb.iter().cloned().collect();
        card.pulse.because = Some("It just landed in Downloads".into());
        card
    }

    /// One-taps: open in the slicer, a print profile, send to the printer
    /// (through the agent, which asks first), and move to the models folder.
    pub fn chips(&self, slicer: Option<&Slicer>, models: &Path, windows: bool) -> Vec<QuickChip> {
        let stem = self.stem();
        let hint = format!("New in Downloads: {} ({})", self.file.name, self.detail());
        let chip = |what: &str, label: String, value: String, kind: ChipKind| QuickChip {
            id: format!("model:{what}:{}", self.file.name),
            label,
            value,
            kind,
            score: 1.0,
            hint: hint.clone(),
            primary: false,
        };
        let path = self.file.path.to_string_lossy();
        let dims = self
            .dims
            .map(dims_line)
            .unwrap_or_else(|| "unknown size".into());
        let mut out = Vec::new();
        if let Some(s) = slicer {
            out.push(chip(
                "open",
                format!("Open {stem} in {}", s.name),
                s.open_line(&self.file.path),
                ChipKind::Shell,
            ));
        }
        let in_slicer = slicer
            .map(|s| format!(" in {}", s.name))
            .unwrap_or_default();
        out.push(chip(
            "profile",
            "Suggest a print profile".into(),
            format!("Suggest a known-good print profile{in_slicer} for {} ({dims}) on my printer. Ask which printer and filament if you don't know them.", self.file.name),
            ChipKind::Chat,
        ));
        out.push(chip(
            "print",
            format!("Print {stem}"),
            format!("Slice {path}{in_slicer} with a known-good profile and send it to my networked printer. Show me the plan and ask before starting the print."),
            ChipKind::Chat,
        ));
        let mv = if windows { "move" } else { "mv" };
        out.push(chip(
            "move",
            "Move to models".into(),
            format!("{mv} \"{path}\" {}", quote(models)),
            ChipKind::Shell,
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("grokhub-model-drop-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cube_stl_binary() -> Vec<u8> {
        let tris: [[[f32; 3]; 3]; 2] = [
            [[0.0, 0.0, 0.0], [20.0, 0.0, 0.0], [20.0, 10.0, 0.0]],
            [[0.0, 0.0, 0.0], [20.0, 10.0, 5.0], [0.0, 10.0, 5.0]],
        ];
        let mut out = vec![0u8; 80];
        out.extend_from_slice(&(tris.len() as u32).to_le_bytes());
        for t in tris {
            out.extend_from_slice(&[0u8; 12]);
            for v in t {
                for c in v {
                    out.extend_from_slice(&c.to_le_bytes());
                }
            }
            out.extend_from_slice(&[0u8; 2]);
        }
        out
    }

    #[test]
    fn only_model_extensions_count_and_old_downloads_never_offer() {
        assert!(is_model("benchy.STL") && is_model("part.3mf") && is_model("a.obj"));
        assert!(is_model("bracket.step") && is_model("bracket.stp"));
        assert!(!is_model("notes.txt") && !is_model("stl") && !is_model("model.stl.zip"));
        let dir = tmp("scan");
        std::fs::write(dir.join("old.stl"), b"x").unwrap();
        std::fs::write(dir.join("tax-return.pdf"), b"x").unwrap();
        let mut seen = BTreeSet::new();
        assert!(scan_new(&dir, &mut seen, true).is_empty());
        std::fs::write(dir.join("benchy.3mf"), b"abc").unwrap();
        std::fs::write(dir.join("invoice.docx"), b"x").unwrap();
        let got = scan_new(&dir, &mut seen, false);
        assert_eq!(
            got,
            vec![ModelFile {
                path: dir.join("benchy.3mf"),
                name: "benchy.3mf".into(),
                bytes: 3
            }]
        );
        assert!(scan_new(&dir, &mut seen, false).is_empty(), "offered once");
        assert_eq!(
            seen.into_iter().collect::<Vec<_>>(),
            vec!["benchy.3mf".to_string(), "old.stl".to_string()],
            "non-model names are never kept"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn downloads_follows_xdg_user_dirs() {
        let home = Path::new("/home/user");
        assert_eq!(
            downloads_dir(home, None),
            PathBuf::from("/home/user/Downloads")
        );
        assert_eq!(
            downloads_dir(
                home,
                Some("# x\nXDG_DOWNLOAD_DIR=\"$HOME/Telechargements\"\n")
            ),
            PathBuf::from("/home/user/Telechargements")
        );
    }

    #[test]
    fn stl_obj_and_3mf_parse_to_a_mesh_and_a_preview() {
        let stl = parse_stl(&cube_stl_binary()).unwrap();
        assert_eq!(stl.tris.len(), 2);
        assert_eq!(stl.size(), Some([20.0, 10.0, 5.0]));
        let ascii = "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 4 0 0\nvertex 0 3 2\nendloop\nendfacet\nendsolid t\n";
        assert_eq!(
            parse_stl(ascii.as_bytes()).unwrap().size(),
            Some([4.0, 3.0, 2.0])
        );
        let obj = parse_obj("v 0 0 0\nv 2 0 0\nv 2 2 0\nv 0 2 1\nf 1/1 2/2 3/3 4/4\n").unwrap();
        assert_eq!(obj.tris.len(), 2, "quad fans into two");
        let mut zip_bytes = Vec::new();
        {
            let mut z = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_bytes));
            let opts = zip::write::SimpleFileOptions::default();
            z.start_file("3D/3dmodel.model", opts).unwrap();
            std::io::Write::write_all(
                &mut z,
                br#"<model><resources><object id="1"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="30" y="0" z="0"/><vertex x="0" y="12" z="7"/></vertices><triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object></resources></model>"#,
            )
            .unwrap();
            z.start_file("Metadata/thumbnail.png", opts).unwrap();
            std::io::Write::write_all(&mut z, b"\x89PNG fake").unwrap();
            z.finish().unwrap();
        }
        let (thumb, mesh) = mesh_for("part.3MF", &zip_bytes);
        assert_eq!(thumb.as_deref(), Some(&b"\x89PNG fake"[..]));
        assert_eq!(mesh.unwrap().size(), Some([30.0, 12.0, 7.0]));
        assert_eq!(mesh_for("bracket.step", b"ISO-10303-21;"), (None, None));
        let rgba = render(&stl, 64).unwrap();
        assert_eq!(rgba.len(), 64 * 64 * 4);
        let drawn = rgba.as_chunks::<4>().0.iter().filter(|p| p[3] == 255).count();
        assert!(drawn > 200 && drawn < 64 * 64, "drawn {drawn}");
        assert_eq!(render(&Mesh::default(), 64), None);
    }

    #[test]
    fn slicers_are_found_on_linux_and_windows_fixtures() {
        let root = tmp("slicers");
        let flat = root.join("flatpak-app");
        std::fs::create_dir_all(flat.join("com.prusa3d.PrusaSlicer")).unwrap();
        let on_path = |b: &str| (b == "orca-slicer").then(|| PathBuf::from("/usr/bin/orca-slicer"));
        let linux = linux_slicers(&on_path, std::slice::from_ref(&flat));
        let names: Vec<&str> = linux.iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["OrcaSlicer", "PrusaSlicer"]);
        let model = Path::new("/home/user/Downloads/benchy.stl");
        assert_eq!(
            linux[0].open_line(model),
            "\"/usr/bin/orca-slicer\" \"/home/user/Downloads/benchy.stl\""
        );
        assert_eq!(
            linux[1].open_line(model),
            "flatpak run com.prusa3d.PrusaSlicer \"/home/user/Downloads/benchy.stl\""
        );
        let pf = root.join("Program Files");
        let creality = pf.join("Creality").join("Creality Print 6.0");
        std::fs::create_dir_all(&creality).unwrap();
        std::fs::write(creality.join("CrealityPrint.exe"), b"").unwrap();
        let cura = pf.join("UltiMaker Cura 5.8.1");
        std::fs::create_dir_all(&cura).unwrap();
        std::fs::write(cura.join("UltiMaker-Cura.exe"), b"").unwrap();
        std::fs::create_dir_all(pf.join("OrcaSlicer")).unwrap();
        let win = windows_slicers(std::slice::from_ref(&pf));
        let names: Vec<&str> = win.iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            vec!["Creality Print", "Cura"],
            "an empty folder is not an install"
        );
        assert_eq!(win[0].program, creality.join("CrealityPrint.exe"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_offer_card_and_one_taps_gate_printing_through_the_agent() {
        let offer = ModelOffer {
            file: ModelFile {
                path: PathBuf::from("/home/user/Downloads/benchy.stl"),
                name: "benchy.stl".into(),
                bytes: 2 * 1024 * 1024 + 100 * 1024,
            },
            dims: Some([60.0, 31.0, 48.0]),
            thumb: Some("model-thumb:benchy.stl:1".into()),
        };
        let orca = Slicer {
            name: "OrcaSlicer",
            program: "/usr/bin/orca-slicer".into(),
            args: vec![],
        };
        let card = offer.card(Some(&orca), 7);
        assert_eq!(card.title, "New model: benchy.stl");
        assert_eq!(
            card.body.as_deref(),
            Some("2.1 MB, 60 × 31 × 48 mm. Open it in OrcaSlicer, get a print profile, or move it to your models folder.")
        );
        assert_eq!(
            card.pulse.image_urls,
            vec!["model-thumb:benchy.stl:1".to_string()]
        );
        let chips = offer.chips(Some(&orca), Path::new("/home/user/Models"), false);
        let labels: Vec<&str> = chips.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Open benchy in OrcaSlicer",
                "Suggest a print profile",
                "Print benchy",
                "Move to models"
            ]
        );
        assert_eq!(
            chips[2].kind,
            ChipKind::Chat,
            "printing goes through the agent's approval"
        );
        assert!(chips[2].value.contains("ask before starting the print"));
        assert_eq!(
            chips[3].value,
            "mv \"/home/user/Downloads/benchy.stl\" \"/home/user/Models\""
        );
        assert_eq!(offer.chips(None, Path::new("/m"), true).len(), 3);
    }
}
