//! Bildschirm abgreifen (GDI) und Fenster/Bildschirme für die Auswahl auflisten.
//! Alle Koordinaten sind physische Pixel relativ zur linken oberen Ecke des
//! virtuellen Bildschirms (alle Monitore zusammen).

use image::RgbaImage;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, EnumDisplayMonitors, GetDC,
    GetDIBits, GetMonitorInfoW, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT,
    DIB_RGB_COLORS, HDC, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST, MonitorFromPoint, SRCCOPY,
};
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetCursorPos, GetSystemMetrics, GetWindowTextW, IsIconic, IsWindowVisible, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

/// Rechteck in physischen Pixeln (relativ zum virtuellen Bildschirm).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PxRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl PxRect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x as f32 && y >= self.y as f32 && x < (self.x + self.w) as f32 && y < (self.y + self.h) as f32
    }

    /// Schnittmenge mit einem Bild der Größe `w`×`h`; `None`, wenn nichts übrig bleibt.
    pub fn clip(&self, w: u32, h: u32) -> Option<PxRect> {
        let x0 = self.x.max(0);
        let y0 = self.y.max(0);
        let x1 = (self.x + self.w).min(w as i32);
        let y1 = (self.y + self.h).min(h as i32);
        (x1 > x0 && y1 > y0).then_some(PxRect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
    }
}

/// Ein Bildschirmfoto aller Monitore samt Fenstern und Monitoren darauf.
pub struct Screen {
    pub image: RgbaImage,
    /// Sichtbare Fenster, oberstes zuerst.
    pub windows: Vec<PxRect>,
    pub monitors: Vec<PxRect>,
}

/// Physische Pixel statt hochskalierter Werte – sonst wäre das Bild auf Bildschirmen mit
/// 125 % oder 150 % unscharf bzw. abgeschnitten. Muss vor dem ersten Fenster passieren.
pub fn make_dpi_aware() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

pub fn virtual_screen() -> PxRect {
    unsafe {
        PxRect {
            x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            w: GetSystemMetrics(SM_CXVIRTUALSCREEN),
            h: GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    }
}

/// Mauszeiger in physischen Pixeln (Bildschirmkoordinaten, nicht relativ zum virtuellen Bildschirm).
pub fn cursor_pos() -> Option<(i32, i32)> {
    let mut p = POINT::default();
    unsafe { GetCursorPos(&mut p).ok()? };
    Some((p.x, p.y))
}

/// Windows-Skalierung (1.0 = 100 %, 1.5 = 150 %) des Bildschirms an einem Punkt
/// in physischen Bildschirmkoordinaten.
pub fn scale_at(x: i32, y: i32) -> f32 {
    unsafe {
        let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_err() || dpi_x == 0 {
            return 1.0;
        }
        dpi_x as f32 / 96.0
    }
}

pub fn grab() -> Result<Screen, String> {
    let area = virtual_screen();
    let image = grab_rect(area)?;
    let to_local = |r: RECT| PxRect {
        x: r.left - area.x,
        y: r.top - area.y,
        w: r.right - r.left,
        h: r.bottom - r.top,
    };
    Ok(Screen {
        image,
        windows: visible_windows().into_iter().map(to_local).collect(),
        monitors: monitors().into_iter().map(to_local).collect(),
    })
}

/// Kopiert einen Bereich des Bildschirms als RGBA-Bild.
fn grab_rect(area: PxRect) -> Result<RgbaImage, String> {
    if area.w <= 0 || area.h <= 0 {
        return Err("Kein Bildschirm gefunden".into());
    }
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return Err("Bildschirm konnte nicht gelesen werden".into());
        }
        let memory = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, area.w, area.h);
        let old = SelectObject(memory, bitmap.into());
        // CAPTUREBLT nimmt auch halbtransparente Fenster (z. B. Tooltips) mit auf.
        let copied = BitBlt(memory, 0, 0, area.w, area.h, Some(screen), area.x, area.y, SRCCOPY | CAPTUREBLT);

        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: area.w,
                // Negativ = Zeilen von oben nach unten.
                biHeight: -area.h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; area.w as usize * area.h as usize * 4];
        let lines = GetDIBits(
            memory,
            bitmap,
            0,
            area.h as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );

        SelectObject(memory, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);

        if copied.is_err() || lines == 0 {
            return Err("Bildschirm konnte nicht gelesen werden".into());
        }
        // BGRA → RGBA, Alpha ist bei GDI undefiniert.
        for px in pixels.chunks_exact_mut(4) {
            px.swap(0, 2);
            px[3] = 255;
        }
        RgbaImage::from_raw(area.w as u32, area.h as u32, pixels).ok_or_else(|| "Bildschirmfoto ungültig".into())
    }
}

/// Sichtbare Hauptfenster von oben nach unten (Z-Reihenfolge), ohne den Desktop.
fn visible_windows() -> Vec<RECT> {
    unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> BOOL {
        let list = &mut *(data.0 as *mut Vec<RECT>);
        if let Some(rect) = window_rect(hwnd) {
            list.push(rect);
        }
        BOOL(1)
    }
    let mut list: Vec<RECT> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut list as *mut _ as isize));
    }
    list
}

fn window_rect(hwnd: HWND) -> Option<RECT> {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return None;
        }
        // Versteckte UWP-Fenster sind „sichtbar“, aber unsichtbar geschaltet (cloaked).
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, size_of::<u32>() as u32);
        if cloaked != 0 {
            return None;
        }
        let mut class = [0u16; 64];
        let len = GetClassNameW(hwnd, &mut class) as usize;
        let class = String::from_utf16_lossy(&class[..len]);
        // Desktop: dafür gibt es den Bildschirm als Rückfall.
        if class == "Progman" || class == "WorkerW" {
            return None;
        }
        let mut title = [0u16; 2];
        let has_title = GetWindowTextW(hwnd, &mut title) > 0;
        if !has_title && class != "Shell_TrayWnd" && class != "Shell_SecondaryTrayWnd" {
            return None;
        }
        // Ohne Schatten-Rand, so wie man das Fenster sieht.
        let mut rect = RECT::default();
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut _ as *mut _,
            size_of::<RECT>() as u32,
        )
        .ok()?;
        (rect.right - rect.left > 8 && rect.bottom - rect.top > 8).then_some(rect)
    }
}

fn monitors() -> Vec<RECT> {
    unsafe extern "system" fn collect(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let list = &mut *(data.0 as *mut Vec<RECT>);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            list.push(info.rcMonitor);
        }
        BOOL(1)
    }
    let mut list: Vec<RECT> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(collect), LPARAM(&mut list as *mut _ as isize));
    }
    list
}
