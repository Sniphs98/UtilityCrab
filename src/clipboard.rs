use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RMENU, VK_RSHIFT, VK_RWIN,
    VK_SHIFT,
};

/// Clipboard-Stand nach dem letzten Lesen durch [`copy_selection`].
static LAST_READ: AtomicU32 = AtomicU32::new(0);

/// Kopiert den aktuell markierten Text (per simuliertem Strg+C) und gibt ihn zurück.
/// Kopiert Strg+C nichts, wird der Inhalt der Zwischenablage nur genommen, wenn er seit dem
/// letzten Mal neu ist (z. B. von Hand kopiert) – sonst würde alter Text erneut vorgelesen.
pub fn copy_selection() -> Result<String, String> {
    // Kurz warten, ob der Benutzer die Tasten vom Kürzel schon loslässt.
    wait_for_modifiers_released(Duration::from_millis(150));

    let before = sequence_number();
    send_ctrl_c();
    let copied = wait_for_change(before, Duration::from_millis(700));
    if !copied && before == LAST_READ.load(Ordering::SeqCst) {
        return Err("Kein Text markiert".into());
    }

    let text = arboard::Clipboard::new()
        .and_then(|mut c| c.get_text())
        .map_err(|e| format!("Kein Text in der Zwischenablage: {e}"))?;
    LAST_READ.store(sequence_number(), Ordering::SeqCst);
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("Kein Text markiert".into());
    }
    Ok(text)
}

pub fn sequence_number() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

/// Wartet, bis sich die Zwischenablage ändert. Gibt `true` zurück, falls das passiert ist.
pub fn wait_for_change(before: u32, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if sequence_number() != before {
            // Programme schreiben oft mehrere Formate nacheinander.
            thread::sleep(Duration::from_millis(50));
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

fn wait_for_modifiers_released(timeout: Duration) {
    let start = Instant::now();
    let modifiers = [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN];
    while start.elapsed() < timeout {
        let any_down = modifiers
            .iter()
            .any(|vk| unsafe { GetAsyncKeyState(vk.0 as i32) } as u16 & 0x8000 != 0);
        if !any_down {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// Schickt Strg+C so, dass es auch dann als Strg+C ankommt, wenn der Benutzer noch
/// Strg+Alt (vom Kürzel) gedrückt hält: Alt, Umschalt und Win werden kurz virtuell
/// losgelassen und danach wieder gedrückt, damit das System den echten Tastenzustand behält.
fn send_ctrl_c() {
    let vk_c = VIRTUAL_KEY(b'C' as u16);
    let held = |vk: VIRTUAL_KEY| unsafe { GetAsyncKeyState(vk.0 as i32) } as u16 & 0x8000 != 0;
    let others: Vec<VIRTUAL_KEY> = [VK_LMENU, VK_RMENU, VK_LSHIFT, VK_RSHIFT, VK_LWIN, VK_RWIN]
        .into_iter()
        .filter(|vk| held(*vk))
        .collect();
    let ctrl_held = held(VK_CONTROL);

    let mut inputs = Vec::new();
    if !others.is_empty() {
        // Eine bedeutungslose Taste verhindert, dass das Loslassen von Alt/Win das Menü
        // bzw. das Startmenü öffnet.
        inputs.extend([key_input(VK_NOOP, false), key_input(VK_NOOP, true)]);
        inputs.extend(others.iter().map(|vk| key_input(*vk, true)));
    }
    if !ctrl_held {
        inputs.push(key_input(VK_CONTROL, false));
    }
    inputs.extend([key_input(vk_c, false), key_input(vk_c, true)]);
    if !ctrl_held {
        inputs.push(key_input(VK_CONTROL, true));
    }
    if !others.is_empty() {
        inputs.extend(others.iter().map(|vk| key_input(*vk, false)));
        inputs.extend([key_input(VK_NOOP, false), key_input(VK_NOOP, true)]);
    }
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

/// Nicht belegter virtueller Tastencode (0xE8), löst nichts aus.
const VK_NOOP: VIRTUAL_KEY = VIRTUAL_KEY(0xE8);

fn key_input(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}
