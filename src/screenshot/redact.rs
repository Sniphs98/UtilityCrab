//! Sensible Daten automatisch finden: Texterkennung mit Wortpositionen, dann Muster
//! (E-Mail, Telefon, IBAN …) auf jede Zeile anwenden.
//!
//! Ein neues Muster = ein Eintrag in [`PATTERNS`]. Es erscheint dann automatisch im Menü
//! „Sensible Daten unkenntlich machen“ und wird gemerkt.

use eframe::egui::{pos2, Rect};
use regex::Regex;

use crate::ocr::Word;

pub struct Pattern {
    /// Dauerhafte ID (für die gemerkte Auswahl).
    pub id: &'static str,
    pub label: &'static str,
    pub regex: &'static str,
    /// Treffer mit weniger Ziffern zählen nicht (Telefonnummern vs. Jahreszahlen).
    pub min_digits: usize,
    pub default_on: bool,
}

pub const PATTERNS: &[Pattern] = &[
    Pattern {
        id: "email",
        label: "E-Mail-Adressen",
        regex: r"[\w.+-]+@[\w-]+(?:\.[\w-]+)+",
        min_digits: 0,
        default_on: true,
    },
    Pattern {
        id: "telefon",
        label: "Telefonnummern",
        regex: r"(?:(?:\+|00)\d{1,3}[\s./-]?)?\(?0?\d{2,5}\)?[\s./-]?\d{2,}(?:[\s./-]?\d{2,})*",
        min_digits: 7,
        default_on: true,
    },
    Pattern {
        id: "iban",
        label: "IBAN",
        regex: r"\b[A-Z]{2}\d{2}(?:\s?[A-Z0-9]{4}){2,7}(?:\s?[A-Z0-9]{1,4})?\b",
        min_digits: 8,
        default_on: true,
    },
    Pattern {
        id: "karte",
        label: "Kreditkartennummern",
        regex: r"\b(?:\d{4}[\s-]?){3}\d{1,4}\b",
        min_digits: 13,
        default_on: true,
    },
    Pattern {
        id: "ip",
        label: "IP-Adressen",
        regex: r"\b(?:\d{1,3}\.){3}\d{1,3}\b",
        min_digits: 4,
        default_on: false,
    },
    Pattern {
        id: "url",
        label: "Links (URLs)",
        regex: r"(?i)\b(?:https?://|www\.)\S+",
        min_digits: 0,
        default_on: false,
    },
    Pattern {
        id: "alles",
        label: "Gesamter Text",
        regex: r"\S+",
        min_digits: 0,
        default_on: false,
    },
];

pub fn default_selection() -> Vec<String> {
    PATTERNS.iter().filter(|p| p.default_on).map(|p| p.id.to_string()).collect()
}

/// Ein Treffer: Bereich im Bild und welches Muster ihn gefunden hat.
pub struct Found {
    pub rect: Rect,
    pub pattern: &'static str,
}

/// Sucht in den erkannten Zeilen nach allen gewählten Mustern.
pub fn find(lines: &[Vec<Word>], enabled: &[String]) -> Vec<Found> {
    let patterns: Vec<(&Pattern, Regex)> = PATTERNS
        .iter()
        .filter(|p| enabled.iter().any(|id| id == p.id))
        .filter_map(|p| Regex::new(p.regex).ok().map(|r| (p, r)))
        .collect();
    let mut found = Vec::new();
    for words in lines {
        // Zeile zusammensetzen und merken, welches Wort an welcher Stelle steht.
        let mut text = String::new();
        let mut spans = Vec::new();
        for word in words {
            if !text.is_empty() {
                text.push(' ');
            }
            let start = text.len();
            text.push_str(&word.text);
            spans.push((start, text.len(), word.rect));
        }
        for (pattern, regex) in &patterns {
            for m in regex.find_iter(&text) {
                if m.as_str().chars().filter(char::is_ascii_digit).count() < pattern.min_digits {
                    continue;
                }
                let rects: Vec<Rect> = spans
                    .iter()
                    .filter(|(s, e, _)| *s < m.end() && *e > m.start())
                    .map(|(_, _, [x, y, w, h])| Rect::from_min_size(pos2(*x, *y), eframe::egui::vec2(*w, *h)))
                    .collect();
                if let Some(first) = rects.first() {
                    let union = rects.iter().fold(*first, |a, b| a.union(*b));
                    // Etwas Rand, damit keine Unterlängen oder Kanten stehen bleiben.
                    let pad = (union.height() * 0.18).max(2.0);
                    found.push(Found {
                        rect: union.expand(pad),
                        pattern: pattern.label,
                    });
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, x: f32) -> Word {
        Word {
            text: text.into(),
            rect: [x, 0.0, 10.0 * text.len() as f32, 12.0],
        }
    }

    #[test]
    fn finds_email_phone_and_iban_across_words() {
        let line = vec![
            word("Mail:", 0.0),
            word("max@example.com", 60.0),
            word("Tel.", 220.0),
            word("+49", 260.0),
            word("170", 300.0),
            word("1234567", 340.0),
        ];
        let iban = vec![word("DE89", 0.0), word("3704", 50.0), word("0044", 100.0), word("0532", 150.0), word("0130", 200.0), word("00", 250.0)];
        let found = find(&[line, iban], &default_selection());
        let labels: Vec<&str> = found.iter().map(|f| f.pattern).collect();
        assert!(labels.contains(&"E-Mail-Adressen"));
        assert!(labels.contains(&"Telefonnummern"));
        assert!(labels.contains(&"IBAN"));
        let phone = found.iter().find(|f| f.pattern == "Telefonnummern").unwrap();
        assert!(phone.rect.min.x <= 260.0 && phone.rect.max.x >= 400.0);
    }

    #[test]
    fn ignores_years() {
        let found = find(&[vec![word("Stand", 0.0), word("2024", 60.0)]], &default_selection());
        assert!(found.is_empty());
    }
}
