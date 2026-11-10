//! Spracherkennung für Text, mit besonderem Blick auf Deutsch und Englisch.
//!
//! Die allgemeine Erkennung von `whatlang` kennt rund 70 Sprachen und ist bei kurzen Sätzen
//! selten „sicher“ – „Hello world.“ hält sie z. B. für Niederländisch. Zwischen Deutsch und
//! Englisch allein lässt sich dagegen auch bei kurzen Texten meist gut unterscheiden.

use whatlang::{Detector, Lang, Script};

/// Erkennt die Sprache des Textes (ISO 639-1) oder `None`, wenn sie unklar ist.
///
/// Zuerst die allgemeine Erkennung; ist sie unsicher, wird nur zwischen Deutsch und Englisch entschieden.
pub fn detect(text: &str) -> Option<&'static str> {
    if let Some(code) = whatlang::detect(text)
        .filter(|i| i.is_reliable())
        .and_then(|i| iso_code(i.lang()))
    {
        return Some(code);
    }
    detect_german_or_english(text)
}

/// Entscheidet zwischen Deutsch (`"de"`) und Englisch (`"en"`); `None`, wenn der Text keins von beiden zu sein scheint.
pub fn detect_german_or_english(text: &str) -> Option<&'static str> {
    if whatlang::detect_script(text) != Some(Script::Latin) {
        return None;
    }
    // Häufige Wörter geben bei sehr kurzen Texten den Ausschlag.
    let (mut de, mut en) = (0, 0);
    for word in text.split(|c: char| !c.is_alphabetic()).filter(|w| !w.is_empty()) {
        let word = word.to_lowercase();
        de += GERMAN_WORDS.contains(&word.as_str()) as i32;
        en += ENGLISH_WORDS.contains(&word.as_str()) as i32;
    }
    let words_say = match de.cmp(&en) {
        std::cmp::Ordering::Greater => Some("de"),
        std::cmp::Ordering::Less => Some("en"),
        std::cmp::Ordering::Equal => None,
    };

    let info = Detector::with_allowlist(vec![Lang::Deu, Lang::Eng])
        .detect(text)
        .map(|i| (i.lang(), i.confidence()));
    let trigrams_say = info
        .filter(|&(_, confidence)| confidence >= MIN_CONFIDENCE)
        .map(|(lang, _)| if lang == Lang::Deu { "de" } else { "en" });

    match (trigrams_say, words_say) {
        // Bei Widerspruch zählen die eindeutigen Wörter mehr als eine knappe Zeichenstatistik.
        (Some(t), Some(w)) if t != w => {
            let confident = info.is_some_and(|(_, confidence)| confidence >= 0.8);
            Some(if confident { t } else { w })
        }
        (Some(t), _) => Some(t),
        (None, w) => w,
    }
}

/// Mindestsicherheit der Deutsch/Englisch-Unterscheidung. Darunter liegen z. B. französische Sätze.
const MIN_CONFIDENCE: f64 = 0.2;

/// Kurze, eindeutige Wörter (kommen in der jeweils anderen Sprache praktisch nicht vor).
const GERMAN_WORDS: &[&str] = &[
    "der", "die", "das", "und", "ist", "nicht", "ein", "eine", "einen", "ich", "du", "wir", "ihr", "sie", "es", "mit",
    "auf", "für", "von", "zu", "den", "dem", "des", "auch", "noch", "nur", "oder", "aber", "wie", "wer",
    "warum", "wo", "bitte", "danke", "ja", "nein", "hallo", "tschüss", "gut", "sehr", "schon", "jetzt", "heute",
    "morgen", "hier", "dort", "mein", "dein", "sind", "bin", "bist", "hat", "habe", "haben", "wird", "werden",
    "kann", "können", "muss", "müssen", "nach", "über", "unter", "zum", "zur", "im", "vom", "beim", "dass",
    "weil", "wenn", "dann", "mal", "doch", "kein", "keine", "alle", "viel", "neu", "neue", "datei", "speichern",
    "öffnen", "einstellungen", "weiter", "zurück", "abbrechen", "fertig", "guten", "tag",
];
const ENGLISH_WORDS: &[&str] = &[
    "the", "and", "is", "are", "not", "a", "of", "to", "you", "we", "they", "he", "she", "it", "with", "on",
    "for", "from", "this", "that", "these", "those", "only", "or", "but", "how", "what", "who", "why",
    "where", "please", "thanks", "thank", "yes", "hello", "hi", "bye", "good", "very", "now", "today", "here",
    "there", "my", "your", "be", "has", "have", "would", "can", "could", "must", "should", "after",
    "about", "under", "if", "then", "no", "all", "much", "new", "file", "save", "open", "settings", "next",
    "back", "cancel", "done", "morning", "click", "do", "does", "did", "were", "been", "just", "great",
];

fn iso_code(lang: Lang) -> Option<&'static str> {
    Some(match lang {
        Lang::Deu => "de",
        Lang::Eng => "en",
        Lang::Fra => "fr",
        Lang::Spa => "es",
        Lang::Ita => "it",
        Lang::Por => "pt",
        Lang::Nld => "nl",
        Lang::Pol => "pl",
        Lang::Ces => "cs",
        Lang::Slk => "sk",
        Lang::Slv => "sl",
        Lang::Hrv => "hr",
        Lang::Hun => "hu",
        Lang::Ron => "ro",
        Lang::Bul => "bg",
        Lang::Ell => "el",
        Lang::Rus => "ru",
        Lang::Ukr => "uk",
        Lang::Tur => "tr",
        Lang::Swe => "sv",
        Lang::Dan => "da",
        Lang::Fin => "fi",
        Lang::Est => "et",
        Lang::Lit => "lt",
        Lang::Lav => "lv",
        Lang::Ind => "id",
        Lang::Vie => "vi",
        Lang::Hin => "hi",
        Lang::Ara => "ar",
        Lang::Kor => "ko",
        Lang::Jpn => "ja",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_english_sentences() {
        for text in [
            "Hello world.",
            "Thanks!",
            "Good morning, how are you?",
            "This is great.",
            "Click here to continue.",
            "Save file",
            "Download the latest version",
        ] {
            assert_eq!(detect(text), Some("en"), "{text}");
        }
    }

    #[test]
    fn short_german_sentences() {
        for text in [
            "Das ist gut.",
            "Hallo zusammen!",
            "Wie geht es dir?",
            "Ich bin müde.",
            "Datei speichern",
            "Einstellungen",
            "Das Wetter ist heute schön.",
            // Wie ihn eine englische Texterkennung ohne Umlaute liest.
            "Gesprach mit meinem Kollegen uber das Projekt",
        ] {
            assert_eq!(detect(text), Some("de"), "{text}");
        }
    }

    #[test]
    fn other_languages_stay_other() {
        assert_eq!(detect("Bonjour, comment allez-vous? Je voudrais un café, s'il vous plaît."), Some("fr"));
        assert_eq!(detect_german_or_english("Bonjour, comment allez-vous?"), None);
        assert_eq!(detect_german_or_english("Привет, как дела? Сегодня хорошая погода."), None);
    }
}
