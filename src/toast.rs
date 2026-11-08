//! Windows-Benachrichtigungen (Toasts), auch mit mitlaufendem Fortschrittsbalken.
//!
//! Ein Programm ohne App-Paket braucht dafür eine registrierte AppUserModelID mit Name und Icon
//! (HKCU, keine Admin-Rechte). Das wird vor jedem Toast kurz sichergestellt.

use windows::core::HSTRING;
use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{NotificationData, ToastNotification, ToastNotificationManager, ToastNotifier};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

const APP_ID: &str = "Sniphs.UtilityCrab";
const GROUP: &str = "utilitycrab";

/// Zeigt einen fertigen Hinweis als Windows-Benachrichtigung.
pub fn show_notice(tag: &str, notice: &crate::popup::Notice) -> Result<(), String> {
    show(tag, &notice.title, &notice.plain_lines().join("\n"))
}

/// Gibt eine Folge von Hinweisen als Toasts aus: laufende als Fortschrittsbalken
/// (erst nach einer Sekunde, damit kurze Aufträge nicht flackern), dann das Ergebnis.
pub struct NoticeSink {
    tag: String,
    started: std::time::Instant,
    /// `Some(None)`: Erstellen ist fehlgeschlagen, nicht erneut versuchen.
    progress: Option<Option<Progress>>,
}

impl NoticeSink {
    pub fn new(tag: &str) -> Self {
        Self {
            tag: tag.into(),
            started: std::time::Instant::now(),
            progress: None,
        }
    }

    /// Fehler nur beim Ergebnis-Toast, damit der Aufrufer auf etwas anderes ausweichen kann.
    pub fn publish(&mut self, notice: &crate::popup::Notice) -> Result<(), String> {
        if notice.kind != crate::popup::Kind::Running {
            return show_notice(&self.tag, notice);
        }
        if self.started.elapsed() < std::time::Duration::from_secs(1) {
            return Ok(());
        }
        let toast = self
            .progress
            .get_or_insert_with(|| progress(&self.tag, &notice.title).ok());
        if let (Some(toast), Some((fraction, label))) = (toast, notice.progress()) {
            toast.update(fraction, label);
        }
        Ok(())
    }
}

/// Ein laufender Fortschritts-Toast; [`Progress::update`] verschiebt den Balken.
pub struct Progress {
    notifier: ToastNotifier,
    tag: String,
    sequence: u32,
}

/// Zeigt einen Toast mit Fortschrittsbalken. Gleicher `tag` ersetzt einen vorhandenen Toast.
pub fn progress(tag: &str, title: &str) -> Result<Progress, String> {
    let xml = format!(
        r#"<toast duration="long"><visual><binding template="ToastGeneric">
             <text>{}</text>
             <progress value="{{value}}" valueStringOverride="{{percent}}" status="{{status}}"/>
           </binding></visual><audio silent="true"/></toast>"#,
        escape(title)
    );
    let notifier = notifier()?;
    let toast = build(&xml, tag).map_err(err)?;
    toast.SetData(&data(0.0, "", 0).map_err(err)?).map_err(err)?;
    notifier.Show(&toast).map_err(err)?;
    Ok(Progress {
        notifier,
        tag: tag.to_string(),
        sequence: 0,
    })
}

impl Progress {
    /// `fraction` zwischen 0 und 1, `status` z. B. „2 von 5 Bildern“.
    pub fn update(&mut self, fraction: f32, status: &str) {
        self.sequence += 1;
        if let Ok(data) = data(fraction, status, self.sequence) {
            let _ = self
                .notifier
                .UpdateWithTagAndGroup(&data, &HSTRING::from(&self.tag), &HSTRING::from(GROUP));
        }
    }
}

/// Einfacher Toast mit Titel und Text. Gleicher `tag` ersetzt z. B. den Fortschritts-Toast.
pub fn show(tag: &str, title: &str, body: &str) -> Result<(), String> {
    let xml = format!(
        r#"<toast><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual></toast>"#,
        escape(title),
        escape(body)
    );
    let toast = build(&xml, tag).map_err(err)?;
    notifier()?.Show(&toast).map_err(err)
}

fn build(xml: &str, tag: &str) -> windows::core::Result<ToastNotification> {
    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(xml))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    toast.SetTag(&HSTRING::from(tag))?;
    toast.SetGroup(&HSTRING::from(GROUP))?;
    Ok(toast)
}

fn data(fraction: f32, status: &str, sequence: u32) -> windows::core::Result<NotificationData> {
    let fraction = fraction.clamp(0.0, 1.0);
    let data = NotificationData::new()?;
    let values = data.Values()?;
    values.Insert(&HSTRING::from("value"), &HSTRING::from(format!("{fraction:.3}")))?;
    values.Insert(&HSTRING::from("percent"), &HSTRING::from(format!("{:.0} %", fraction * 100.0)))?;
    values.Insert(&HSTRING::from("status"), &HSTRING::from(status))?;
    data.SetSequenceNumber(sequence)?;
    Ok(data)
}

fn notifier() -> Result<ToastNotifier, String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    register()?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID)).map_err(err)
}

/// Name und Icon, die Windows im Toast und in den Benachrichtigungseinstellungen zeigt.
fn register() -> Result<(), String> {
    let icon = crate::data_dir().join("krabbe.png");
    if !icon.exists() {
        let _ = std::fs::create_dir_all(crate::data_dir());
        let size = 128;
        image::save_buffer(
            &icon,
            &crate::crab_pixels::crab_rgba(size),
            size,
            size,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| format!("Toast-Icon: {e}"))?;
    }
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(format!(r"Software\Classes\AppUserModelId\{APP_ID}"))
        .map_err(|e| format!("Toast registrieren: {e}"))?;
    key.set_value("DisplayName", &"UtilityCrab")
        .and_then(|_| key.set_value("IconUri", &icon.to_string_lossy().into_owned()))
        .map_err(|e| format!("Toast registrieren: {e}"))
}

/// Entfernt die Registrierung wieder (Deinstallation).
pub fn unregister() {
    let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(format!(r"Software\Classes\AppUserModelId\{APP_ID}"));
}

fn err(e: windows::core::Error) -> String {
    format!("Benachrichtigung: {e}")
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
