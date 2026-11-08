use tray_icon::Icon;

pub const SIZE: u32 = 32;

pub fn crab_icon() -> Icon {
    Icon::from_rgba(crab_rgba(), SIZE, SIZE).expect("Icon ungültig")
}

/// Orangefarbenes Krabben-Icon (RGBA, `SIZE`×`SIZE`), damit keine Bilddatei nötig ist.
pub fn crab_rgba() -> Vec<u8> {
    crate::crab_pixels::crab_rgba(SIZE)
}
