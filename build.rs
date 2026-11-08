//! Bettet das App-Icon (`assets/icon.png`) als .ico in die .exe ein
//! (Explorer, Startmenü, Kontextmenü „Konvertieren“) und reicht die Release-Version weiter.

use std::path::PathBuf;

use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::imageops::FilterType;
use image::ExtendedColorType;

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.png");
    // Release-Version (z. B. 0.1.0.7) vom Workflow bzw. installer/build.ps1; leer = lokaler Build.
    println!("cargo:rerun-if-env-changed=UTILITYCRAB_VERSION");
    let version = std::env::var("UTILITYCRAB_VERSION").unwrap_or_default();
    println!("cargo:rustc-env=UTILITYCRAB_VERSION={version}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let source = image::open("assets/icon.png")
        .expect("assets/icon.png fehlt oder ist kein Bild")
        .to_rgba8();
    let frames: Vec<IcoFrame> = [16, 20, 24, 32, 40, 48, 64, 256]
        .into_iter()
        .map(|size| {
            let resized = image::imageops::resize(&source, size, size, FilterType::Lanczos3);
            IcoFrame::as_png(resized.as_raw(), size, size, ExtendedColorType::Rgba8).expect("Icon-Größe")
        })
        .collect();

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let ico = out.join("crab.ico");
    let file = std::fs::File::create(&ico).expect("crab.ico anlegen");
    IcoEncoder::new(file).encode_images(&frames).expect("crab.ico schreiben");

    let mut res = winresource::WindowsResource::new();
    res.set_icon(ico.to_str().unwrap());
    res.compile().expect("Ressourcen konnten nicht kompiliert werden (rc.exe aus dem Windows SDK nötig)");
}
