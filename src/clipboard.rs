use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};

/// Kopiert den aktuell markierten Text (per simuliertem Strg+C) und gibt ihn zurück.
/// Wurde nichts Neues kopiert, wird der bisherige Inhalt der Zwischenablage verwendet.
pub fn copy_selection() -> Result<String, String> {
    // Solange der Benutzer noch Strg/Alt vom Shortcut hält, würde Strg+C
    // als Strg+Alt+C ankommen – also kurz warten, bis alles losgelassen ist.
    wait_for_modifiers_released(Duration::from_secs(1));

    let before = sequence_number();
    send_ctrl_c();
    wait_for_change(before, Duration::from_millis(500));

    let text = arboard::Clipboard::new()
        .and_then(|mut c| c.get_text())
        .map_err(|e| format!("Kein Text in der Zwischenablage: {e}"))?;
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

fn send_ctrl_c() {
    let vk_c = VIRTUAL_KEY(b'C' as u16);
    let inputs = [
        key_input(VK_CONTROL, false),
        key_input(vk_c, false),
        key_input(vk_c, true),
        key_input(VK_CONTROL, true),
    ];
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

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
