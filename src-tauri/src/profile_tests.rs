use super::*;

fn dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "grokhub-shell-profile-{tag}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("temp dir");
    d
}

fn app_json(d: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(d.join("app.json")).unwrap_or_default())
        .unwrap_or(Value::Null)
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut img = image::RgbaImage::new(w, h);
    // The middle column is red, so a centered crop keeps it.
    for y in 0..h {
        img.put_pixel(w / 2, y, image::Rgba([255, 0, 0, 255]));
    }
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

fn installed_size(d: &Path) -> (u32, u32) {
    let img = image::open(d.join(PICTURE_FILE)).expect("profile.png");
    (img.width(), img.height())
}

#[test]
fn the_shown_name_follows_the_cabins_order() {
    assert_eq!(shown_name("Pat", Some("Robin Lee"), ""), "Pat");
    // The Grok name and memory give a first name, as the cabin's greeting does.
    assert_eq!(shown_name("  ", Some("Robin Lee"), ""), "Robin");
    assert_eq!(shown_name("", Some("Grok"), "- Name: Sam\n"), "Sam");
    assert_eq!(shown_name("", None, "- Name: Sam\n"), "Sam");
    assert_eq!(shown_name("", Some(""), "- Name: Sam\n"), "Sam");
    assert_eq!(shown_name("", None, ""), "You");
}

#[test]
fn a_long_name_is_cut_to_64_characters() {
    let long = format!("  {}  ", "a".repeat(70));
    assert_eq!(clip_name(&long), "a".repeat(64));
}

#[test]
fn the_name_saves_beside_other_settings_and_a_blank_one_goes() {
    let d = dir("name");
    std::fs::write(d.join("app.json"), r#"{"weeklySpendCapUsd":5}"#).unwrap();
    set_name(&d, "  Pat ").unwrap();
    assert_eq!(
        app_json(&d),
        json!({ "weeklySpendCapUsd": 5, "displayName": "Pat" })
    );
    let p = load(&d, Some("Robin Lee"));
    assert_eq!(
        p,
        Profile {
            name: "Pat".into(),
            shown_name: "Pat".into(),
            initial: "P".into(),
            picture: None,
        }
    );
    set_name(&d, "   ").unwrap();
    assert_eq!(app_json(&d), json!({ "weeklySpendCapUsd": 5 }));
    assert_eq!(load(&d, Some("Robin Lee")).shown_name, "Robin");
}

#[test]
fn an_unreadable_app_json_is_left_alone() {
    let d = dir("bad");
    std::fs::write(d.join("app.json"), "not json").unwrap();
    assert!(set_name(&d, "Pat").is_err());
    assert_eq!(
        std::fs::read_to_string(d.join("app.json")).unwrap(),
        "not json"
    );
}

#[test]
fn a_wide_picture_is_cropped_square_from_the_middle() {
    let d = dir("crop");
    let src = d.join("wide.png");
    std::fs::write(&src, png(8, 4)).unwrap();
    install_picture(&d, &src).unwrap();
    assert_eq!(installed_size(&d), (4, 4));
    let img = image::open(d.join(PICTURE_FILE)).unwrap().to_rgba8();
    assert_eq!(img.get_pixel(2, 0), &image::Rgba([255, 0, 0, 255]));
    assert_eq!(
        app_json(&d)["profilePicture"],
        json!(d.join("profile.png").to_string_lossy())
    );
    let p = load(&d, None);
    assert!(
        p.picture
            .as_deref()
            .is_some_and(|s| s.starts_with("data:image/png;base64,iVBORw0KGgo")),
        "{:?}",
        p.picture
    );
}

#[test]
fn a_big_jpeg_becomes_256_pixels() {
    let d = dir("jpeg");
    let src = d.join("big.jpg");
    let img = image::DynamicImage::ImageRgb8(image::RgbImage::new(600, 300));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
    std::fs::write(&src, out.into_inner()).unwrap();
    install_picture(&d, &src).unwrap();
    assert_eq!(installed_size(&d), (256, 256));
}

#[test]
fn files_that_are_not_pictures_are_refused_and_nothing_is_written() {
    let d = dir("refuse");
    let txt = d.join("note.txt");
    std::fs::write(&txt, "hello").unwrap();
    assert_eq!(install_picture(&d, &txt), Err(NOT_AN_IMAGE.to_string()));
    assert_eq!(
        install_picture(&d, &d.join("missing.png")),
        Err(TOO_BIG.to_string())
    );
    let empty = d.join("empty.png");
    std::fs::write(&empty, b"").unwrap();
    assert_eq!(install_picture(&d, &empty), Err(TOO_BIG.to_string()));
    // A tiny PNG whose header claims 50000 by 50000.
    let mut bomb = png(1, 1);
    bomb[16..20].copy_from_slice(&50_000u32.to_be_bytes());
    bomb[20..24].copy_from_slice(&50_000u32.to_be_bytes());
    let bomb_path = d.join("bomb.png");
    std::fs::write(&bomb_path, bomb).unwrap();
    assert_eq!(
        install_picture(&d, &bomb_path),
        Err(NOT_AN_IMAGE.to_string())
    );
    assert!(!d.join(PICTURE_FILE).exists());
    assert!(!d.join("app.json").exists());
}

#[test]
fn remove_drops_the_key_and_the_file() {
    let d = dir("clear");
    let src = d.join("a.png");
    std::fs::write(&src, png(4, 4)).unwrap();
    install_picture(&d, &src).unwrap();
    set_name(&d, "Pat").unwrap();
    clear_picture(&d).unwrap();
    assert!(!d.join(PICTURE_FILE).exists());
    assert_eq!(app_json(&d), json!({ "displayName": "Pat" }));
    assert_eq!(load(&d, None).picture, None);
    clear_picture(&d).unwrap();
}

#[test]
fn only_the_config_folders_picture_is_ever_read() {
    let d = dir("path");
    let elsewhere = d.join("elsewhere.png");
    std::fs::write(&elsewhere, png(4, 4)).unwrap();
    std::fs::write(
        d.join("app.json"),
        json!({ "profilePicture": elsewhere.to_string_lossy() }).to_string(),
    )
    .unwrap();
    assert_eq!(load(&d, None).picture, None);
}
