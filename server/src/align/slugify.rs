//! Slugify for alignment search / CTC baselines.
//!
//! Storyteller: locale transliteration + OpenCC Han fold + number→words + Mapping.
//! Shelf wave C: NFKD Latin fold + hand Cyrillic map + en/fr/ru cardinals + Mapping.
//!
//! Unicode scalars (BMP ≡ ST UTF-16 for EN); grapheme clusters not used in ST
//! either. Currency: Intl.Locale.maximize + locale-currency shaped table — EN
//! `$£€`, euro `€`, ja/zh `¥`, ko `₩`, plus rarer sv/no/da `kr`, pl `zł`, cs
//! `Kč`, ru `₽`, pt-BR `R$`, hi `₹`, tr `₺`, th `฿`, he `₪`, hu `Ft`, uk `₴`.
//! Sentence CLDR suppressions live in `suppressions.rs` / `markup.rs`.

use std::collections::HashMap;
use std::sync::OnceLock;

use unicode_normalization::UnicodeNormalization;

use super::mapping::{MapAssoc, Mapping};

/// OpenCC traditional→simplified (ST `TSCharacters.json`).
fn traditional_to_simplified() -> &'static HashMap<String, String> {
    static MAP: OnceLock<HashMap<String, String>> = OnceLock::new();
    MAP.get_or_init(|| {
        serde_json::from_str(include_str!("opencc/TSCharacters.json"))
            .expect("opencc/TSCharacters.json")
    })
}

/// ST slugify.ts Han fold: when vocab lacks traditional `c` but has simplified, replace.
pub fn fold_han_for_vocab(text: &str, vocab: &HashMap<String, i32>) -> String {
    let map = traditional_to_simplified();
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        let key = ch.to_string();
        if let Some(simplified) = map.get(&key) {
            if !vocab.contains_key(&key) && vocab.contains_key(simplified) {
                out.push_str(simplified);
                continue;
            }
        }
        out.push(ch);
    }
    out
}

/// Result of locale-aware slugify (ST `{ result, mapping }`).
#[derive(Debug, Clone)]
pub struct SlugifyOutcome {
    pub result: String,
    pub mapping: Mapping,
}

/// Lowercase, strip punctuation, collapse whitespace to `-` (Storyteller search delimiter).
pub fn slugify_for_search(text: &str) -> String {
    slugify_for_search_with_locale(text, "en")
}

/// Locale-aware search slug (`-` separator): numbers→words + transliterate + ASCII filter.
pub fn slugify_for_search_with_locale(text: &str, locale: &str) -> String {
    slugify_with(text, locale, "-").result
}

/// Same as search slugify but returns offset [`Mapping`].
pub fn slugify_for_search_mapped(text: &str, locale: &str) -> SlugifyOutcome {
    slugify_with(text, locale, "-")
}

/// Empty-separator slug (CTC / Viterbi path in ST walkthrough).
pub fn slugify_joined(text: &str, locale: &str) -> String {
    slugify_with(text, locale, "").result
}

/// Empty-separator slug with mapping.
pub fn slugify_joined_mapped(text: &str, locale: &str) -> SlugifyOutcome {
    slugify_with(text, locale, "")
}

fn locale_lang(locale: &str) -> &'static str {
    let primary = locale
        .split(['-', '_'])
        .next()
        .unwrap_or("en")
        .to_ascii_lowercase();
    match primary.as_str() {
        "fr" => "fr",
        "ru" => "ru",
        // Euro-trailing locales: keep distinct for currency; cardinals fall to EN.
        "de" => "de",
        "es" => "es",
        "it" => "it",
        "pt" => "pt",
        "nl" => "nl",
        // Leading yen/won; cardinals fall to EN tables.
        "ja" => "ja",
        "zh" => "zh",
        "ko" => "ko",
        // Rarer maximize currencies (cardinals → EN).
        "sv" => "sv",
        "nb" | "nn" | "no" => "nb",
        "da" => "da",
        "pl" => "pl",
        "cs" => "cs",
        "hi" => "hi",
        "tr" => "tr",
        "th" => "th",
        "he" => "he",
        "hu" => "hu",
        "ro" => "ro",
        "uk" => "uk",
        _ => "en",
    }
}

fn locale_region(locale: &str) -> Option<String> {
    let parts: Vec<&str> = locale.split(['-', '_']).collect();
    parts.get(1).map(|r| r.to_ascii_uppercase())
}

/// ST `Intl.Locale.maximize().region` defaults for bare language tags.
fn maximize_region(lang: &str, region: Option<&str>) -> &'static str {
    if let Some(r) = region {
        // Keep caller region when present (pt-BR vs pt-PT).
        return match r {
            "BR" | "PT" | "US" | "GB" | "CA" | "AU" | "MX" | "TW" | "SE" | "NO" | "DK" | "PL"
            | "CZ" | "RU" | "IN" | "TR" | "TH" | "IL" | "HU" | "RO" | "UA" | "JP" | "KR" | "CN"
            | "DE" | "FR" | "ES" | "IT" | "NL" => {
                // Return static via match on known codes.
                match r {
                    "BR" => "BR",
                    "PT" => "PT",
                    "US" => "US",
                    "GB" => "GB",
                    "CA" => "CA",
                    "AU" => "AU",
                    "MX" => "MX",
                    "TW" => "TW",
                    "SE" => "SE",
                    "NO" => "NO",
                    "DK" => "DK",
                    "PL" => "PL",
                    "CZ" => "CZ",
                    "RU" => "RU",
                    "IN" => "IN",
                    "TR" => "TR",
                    "TH" => "TH",
                    "IL" => "IL",
                    "HU" => "HU",
                    "RO" => "RO",
                    "UA" => "UA",
                    "JP" => "JP",
                    "KR" => "KR",
                    "CN" => "CN",
                    "DE" => "DE",
                    "FR" => "FR",
                    "ES" => "ES",
                    "IT" => "IT",
                    "NL" => "NL",
                    _ => "US",
                }
            }
            _ => match lang {
                "pt" => "PT",
                "en" => "US",
                "sv" => "SE",
                "nb" | "nn" | "no" => "NO",
                "da" => "DK",
                "pl" => "PL",
                "cs" => "CZ",
                "ru" => "RU",
                "hi" => "IN",
                "tr" => "TR",
                "th" => "TH",
                "he" => "IL",
                "hu" => "HU",
                "ro" => "RO",
                "uk" => "UA",
                "ja" => "JP",
                "ko" => "KR",
                "zh" => "CN",
                "de" => "DE",
                "fr" => "FR",
                "es" => "ES",
                "it" => "IT",
                "nl" => "NL",
                _ => "US",
            },
        };
    }
    match lang {
        "pt" => "PT",
        "en" => "US",
        "sv" => "SE",
        "nb" | "nn" | "no" => "NO",
        "da" => "DK",
        "pl" => "PL",
        "cs" => "CZ",
        "ru" => "RU",
        "hi" => "IN",
        "tr" => "TR",
        "th" => "TH",
        "he" => "IL",
        "hu" => "HU",
        "ro" => "RO",
        "uk" => "UA",
        "ja" => "JP",
        "ko" => "KR",
        "zh" => "CN",
        "de" => "DE",
        "fr" => "FR",
        "es" => "ES",
        "it" => "IT",
        "nl" => "NL",
        _ => "US",
    }
}

/// Currency profile from maximized region (ST getCurrency + formatToParts shape).
#[derive(Clone, Copy)]
struct CurrencyProfile {
    /// Symbols longest-first (e.g. `["R$", "$"]` or `["kr.", "kr"]`).
    symbols: &'static [&'static str],
    leading: bool,
    major: &'static str,
    major_plural: &'static str,
    minor: &'static str,
    minor_plural: &'static str,
    /// Skip pluralizing major (yen/won/baht/forint).
    invariant_major: bool,
}

fn currency_for_locale(locale: &str) -> CurrencyProfile {
    let region_owned = locale_region(locale);
    let region = maximize_region(locale_lang(locale), region_owned.as_deref());
    match region {
        "SE" | "NO" => CurrencyProfile {
            symbols: &["kr"],
            leading: false,
            major: "krona",
            major_plural: "kronor",
            minor: "ore",
            minor_plural: "ore",
            invariant_major: false,
        },
        "DK" => CurrencyProfile {
            symbols: &["kr.", "kr"],
            leading: false,
            major: "krone",
            major_plural: "kroner",
            minor: "ore",
            minor_plural: "ore",
            invariant_major: false,
        },
        "PL" => CurrencyProfile {
            symbols: &["zł"],
            leading: false,
            major: "zloty",
            major_plural: "zlotys",
            minor: "grosz",
            minor_plural: "groszy",
            invariant_major: false,
        },
        "CZ" => CurrencyProfile {
            symbols: &["Kč"],
            leading: false,
            major: "koruna",
            major_plural: "korunas",
            minor: "heller",
            minor_plural: "hellers",
            invariant_major: false,
        },
        "RU" => CurrencyProfile {
            symbols: &["₽"],
            leading: false,
            major: "ruble",
            major_plural: "rubles",
            minor: "kopeck",
            minor_plural: "kopecks",
            invariant_major: false,
        },
        "BR" => CurrencyProfile {
            symbols: &["R$", "$"],
            leading: true,
            major: "real",
            major_plural: "reals",
            minor: "centavo",
            minor_plural: "centavos",
            invariant_major: false,
        },
        "IN" => CurrencyProfile {
            symbols: &["₹"],
            leading: true,
            major: "rupee",
            major_plural: "rupees",
            minor: "paisa",
            minor_plural: "paise",
            invariant_major: false,
        },
        "TR" => CurrencyProfile {
            symbols: &["₺"],
            leading: true,
            major: "lira",
            major_plural: "liras",
            minor: "kurus",
            minor_plural: "kurus",
            invariant_major: false,
        },
        "TH" => CurrencyProfile {
            symbols: &["฿"],
            leading: true,
            major: "baht",
            major_plural: "baht",
            minor: "satang",
            minor_plural: "satang",
            invariant_major: true,
        },
        "IL" => CurrencyProfile {
            symbols: &["₪"],
            leading: false,
            major: "shekel",
            major_plural: "shekels",
            minor: "agora",
            minor_plural: "agoras",
            invariant_major: false,
        },
        "HU" => CurrencyProfile {
            symbols: &["Ft"],
            leading: false,
            major: "forint",
            major_plural: "forint",
            minor: "filler",
            minor_plural: "filler",
            invariant_major: true,
        },
        "RO" => CurrencyProfile {
            symbols: &["RON", "lei"],
            leading: false,
            major: "leu",
            major_plural: "lei",
            minor: "ban",
            minor_plural: "bani",
            invariant_major: false,
        },
        "UA" => CurrencyProfile {
            symbols: &["₴"],
            leading: false,
            major: "hryvnia",
            major_plural: "hryvnias",
            minor: "kopiyka",
            minor_plural: "kopiykas",
            invariant_major: false,
        },
        "JP" | "CN" => CurrencyProfile {
            symbols: &["¥", "￥"],
            leading: true,
            major: "yen",
            major_plural: "yen",
            minor: "sen",
            minor_plural: "sen",
            invariant_major: true,
        },
        "KR" => CurrencyProfile {
            symbols: &["₩"],
            leading: true,
            major: "won",
            major_plural: "won",
            minor: "jeon",
            minor_plural: "jeon",
            invariant_major: true,
        },
        "GB" => CurrencyProfile {
            symbols: &["£", "€", "$"],
            leading: true,
            major: "pound",
            major_plural: "pounds",
            minor: "penny",
            minor_plural: "pence",
            invariant_major: false,
        },
        // Eurozone default for de/fr/es/it/nl/pt-PT.
        "DE" | "FR" | "ES" | "IT" | "NL" | "PT" => CurrencyProfile {
            symbols: &["€"],
            leading: false,
            major: "euro",
            major_plural: "euros",
            minor: "cent",
            minor_plural: "cents",
            invariant_major: false,
        },
        // en-US / en-CA / en-AU / es-MX / default: leading $ (+ £ €).
        _ => CurrencyProfile {
            symbols: &["$", "£", "€"],
            leading: true,
            major: "dollar",
            major_plural: "dollars",
            minor: "cent",
            minor_plural: "cents",
            invariant_major: false,
        },
    }
}

/// EN/GB £ and EN € overrides when symbol matched under default $ profile.
fn unit_for_symbol(sym: &str, profile: CurrencyProfile) -> CurrencyProfile {
    match sym {
        "£" => CurrencyProfile {
            symbols: &["£"],
            leading: true,
            major: "pound",
            major_plural: "pounds",
            minor: "penny",
            minor_plural: "pence",
            invariant_major: false,
        },
        "€" => CurrencyProfile {
            symbols: &["€"],
            leading: profile.leading,
            major: "euro",
            major_plural: "euros",
            minor: "cent",
            minor_plural: "cents",
            invariant_major: false,
        },
        _ => profile,
    }
}

/// Core: number→words → transliterate → lowercase a-z + separator gaps.
///
/// Mapping uses original Unicode-scalar offsets (ST walkthrough step 4 shape).
fn slugify_with(text: &str, locale: &str, separator: &str) -> SlugifyOutcome {
    let lang = locale_lang(locale);
    let chars: Vec<char> = text.chars().collect();
    let mut mapping = Mapping::new();
    let mut out = String::new();
    let mut prev_sep = true;
    let mut i = 0usize;

    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            let num_str: String = chars[start..i].iter().collect();
            let n: u64 = num_str.parse().unwrap_or(0);
            let words = number_to_words(n, lang);
            let roman = transliterate(&words);
            let tokens: Vec<String> = roman
                .split(|c: char| !c.is_ascii_alphabetic())
                .filter(|t| !t.is_empty())
                .map(|t| t.to_ascii_lowercase())
                .collect();
            let piece = join_tokens(&tokens, separator);
            let emit = emit_piece(&mut out, &piece, separator, &mut prev_sep, true);
            mapping.insert_map(start as i32, (i - start) as i32, emit);
            continue;
        }

        let orig = i as i32;
        let ch = chars[i];
        i += 1;
        let piece = transliterate_char(ch);
        // ъ/ь and similar: transliterate to empty — delete without a token break.
        if piece.is_empty() {
            mapping.insert_map(orig, 1, 0);
            continue;
        }
        let kept: String = piece
            .chars()
            .filter(|c| c.is_ascii_alphabetic())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        if kept.is_empty() {
            if !separator.is_empty() && !prev_sep {
                let sep_len = separator.chars().count() as i32;
                mapping.insert_map(orig, 1, sep_len);
                out.push_str(separator);
                prev_sep = true;
            } else {
                mapping.insert_map(orig, 1, 0);
            }
            continue;
        }
        // Letter runs continue the current token (no leading sep).
        let emit = emit_piece(&mut out, &kept, separator, &mut prev_sep, false);
        mapping.insert_map(orig, 1, emit);
    }

    if !separator.is_empty() {
        while out.ends_with(separator) {
            out.truncate(out.len() - separator.len());
        }
    }

    SlugifyOutcome {
        result: out,
        mapping,
    }
}

fn join_tokens(tokens: &[String], separator: &str) -> String {
    if separator.is_empty() {
        tokens.concat()
    } else {
        tokens.join(separator)
    }
}

fn emit_piece(
    out: &mut String,
    piece: &str,
    separator: &str,
    prev_sep: &mut bool,
    new_token: bool,
) -> i32 {
    if piece.is_empty() {
        return 0;
    }
    let mut n = 0i32;
    if new_token && !separator.is_empty() && !*prev_sep {
        out.push_str(separator);
        n += separator.chars().count() as i32;
    }
    out.push_str(piece);
    n += piece.chars().count() as i32;
    *prev_sep = false;
    n
}

/// Transliterate keeping case/spaces/punct; strip Latin diacritics; romanize Cyrillic.
///
/// shortcut: Unicode scalars only (no grapheme clusters).
pub fn transliterate(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        out.push_str(&transliterate_char(ch));
    }
    out
}

fn transliterate_char(ch: char) -> String {
    if let Some(cyr) = cyrillic_roman(ch) {
        return cyr.to_string();
    }
    let nfkd: String = ch.nfkd().collect();
    let mut base = String::new();
    for c in nfkd.chars() {
        if unicode_normalization::char::is_combining_mark(c) {
            continue;
        }
        base.push(c);
    }
    base
}

fn cyrillic_roman(ch: char) -> Option<&'static str> {
    // ST @storyteller-platform/transliteration ^4.0.6 (walkthrough step 2).
    Some(match ch {
        'А' => "A",
        'а' => "a",
        'Б' => "B",
        'б' => "b",
        'В' => "V",
        'в' => "v",
        'Г' => "G",
        'г' => "g",
        'Д' => "D",
        'д' => "d",
        'Е' => "E",
        'е' => "e",
        'Ё' => "Yo",
        'ё' => "yo",
        'Ж' => "Zh",
        'ж' => "zh",
        'З' => "Z",
        'з' => "z",
        'И' => "I",
        'и' => "i",
        'Й' => "Y",
        'й' => "y",
        'К' => "K",
        'к' => "k",
        'Л' => "L",
        'л' => "l",
        'М' => "M",
        'м' => "m",
        'Н' => "N",
        'н' => "n",
        'О' => "O",
        'о' => "o",
        'П' => "P",
        'п' => "p",
        'Р' => "R",
        'р' => "r",
        'С' => "S",
        'с' => "s",
        'Т' => "T",
        'т' => "t",
        'У' => "U",
        'у' => "u",
        'Ф' => "F",
        'ф' => "f",
        'Х' => "H",
        'х' => "h",
        'Ц' => "C",
        'ц' => "c",
        'Ч' => "Ch",
        'ч' => "ch",
        'Ш' => "Sh",
        'ш' => "sh",
        'Щ' => "Shch",
        'щ' => "shch",
        'Ъ' | 'ъ' | 'Ь' | 'ь' => "",
        'Ы' => "Y",
        'ы' => "y",
        'Э' => "E",
        'э' => "e",
        'Ю' => "Yu",
        'ю' => "yu",
        'Я' => "Ya",
        'я' => "ya",
        _ => return None,
    })
}

// --- number → words (to-words-shaped en/fr/ru cardinals) ---------------------

pub fn number_to_words(n: u64, lang: &str) -> String {
    match lang {
        "fr" => title_fr(&fr_cardinal(n)),
        "ru" => title_words(&ru_cardinal(n)),
        _ => title_words(&en_cardinal(n)),
    }
}

fn title_words(s: &str) -> String {
    s.split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn title_fr(s: &str) -> String {
    let mut out = String::new();
    let mut cap = true;
    for ch in s.chars() {
        if cap && ch.is_alphabetic() {
            for c in ch.to_uppercase() {
                out.push(c);
            }
            cap = false;
        } else {
            out.push(ch);
            cap = ch == '-' || ch == ' ';
        }
    }
    out
}

fn en_cardinal(n: u64) -> String {
    if n == 0 {
        return "zero".into();
    }
    let ones = [
        "",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
    ];
    let tens = [
        "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
    ];
    fn under_100(n: u64, ones: &[&str], tens: &[&str]) -> String {
        if n < 20 {
            ones[n as usize].into()
        } else {
            let t = tens[(n / 10) as usize];
            let o = n % 10;
            if o == 0 {
                t.into()
            } else {
                format!("{t} {}", ones[o as usize])
            }
        }
    }
    if n < 100 {
        return under_100(n, &ones, &tens);
    }
    if n < 1000 {
        let h = n / 100;
        let r = n % 100;
        if r == 0 {
            format!("{} hundred", ones[h as usize])
        } else {
            format!(
                "{} hundred {}",
                ones[h as usize],
                under_100(r, &ones, &tens)
            )
        }
    } else if n < 1_000_000 {
        let th = n / 1000;
        let r = n % 1000;
        let head = en_cardinal(th);
        if r == 0 {
            format!("{head} thousand")
        } else {
            format!("{head} thousand {}", en_cardinal(r))
        }
    } else {
        // shortcut: huge magnitudes stay numeric
        n.to_string()
    }
}

fn fr_cardinal(n: u64) -> String {
    if n == 0 {
        return "zéro".into();
    }
    let ones = [
        "", "un", "deux", "trois", "quatre", "cinq", "six", "sept", "huit", "neuf", "dix", "onze",
        "douze", "treize", "quatorze", "quinze", "seize",
    ];
    fn under_100(n: u64, ones: &[&str]) -> String {
        if n < 17 {
            return ones[n as usize].into();
        }
        if n < 20 {
            return format!("dix-{}", ones[(n - 10) as usize]);
        }
        let tens_w = [
            "",
            "",
            "vingt",
            "trente",
            "quarante",
            "cinquante",
            "soixante",
        ];
        if n < 70 {
            let t = n / 10;
            let o = n % 10;
            if o == 0 {
                tens_w[t as usize].into()
            } else if o == 1 {
                format!("{} et un", tens_w[t as usize])
            } else {
                format!("{}-{}", tens_w[t as usize], ones[o as usize])
            }
        } else if n < 80 {
            let r = n - 60;
            if r == 11 {
                "soixante et onze".into()
            } else if r < 17 {
                format!("soixante-{}", ones[r as usize])
            } else {
                format!("soixante-dix-{}", ones[(r - 10) as usize])
            }
        } else if n == 80 {
            "quatre-vingts".into()
        } else if n < 100 {
            let r = n - 80;
            if r == 1 {
                "quatre-vingt-un".into()
            } else if r < 17 {
                format!("quatre-vingt-{}", ones[r as usize])
            } else {
                format!("quatre-vingt-dix-{}", ones[(r - 10) as usize])
            }
        } else {
            n.to_string()
        }
    }
    if n < 100 {
        return under_100(n, &ones);
    }
    if n < 1000 {
        let h = n / 100;
        let r = n % 100;
        if r == 0 {
            if h == 1 {
                "cent".into()
            } else {
                format!("{} cents", ones[h as usize])
            }
        } else if h == 1 {
            format!("cent {}", under_100(r, &ones))
        } else {
            format!("{} cent {}", ones[h as usize], under_100(r, &ones))
        }
    } else if n < 1_000_000 {
        let th = n / 1000;
        let r = n % 1000;
        let head = if th == 1 {
            "mille".to_string()
        } else {
            format!("{} mille", under_100(th, &ones))
        };
        if r == 0 {
            head
        } else {
            format!("{head} {}", fr_cardinal(r))
        }
    } else {
        n.to_string()
    }
}

fn ru_cardinal(n: u64) -> String {
    if n == 0 {
        return "ноль".into();
    }
    let ones_m = [
        "",
        "один",
        "два",
        "три",
        "четыре",
        "пять",
        "шесть",
        "семь",
        "восемь",
        "девять",
    ];
    let teens = [
        "десять",
        "одиннадцать",
        "двенадцать",
        "тринадцать",
        "четырнадцать",
        "пятнадцать",
        "шестнадцать",
        "семнадцать",
        "восемнадцать",
        "девятнадцать",
    ];
    let tens = [
        "",
        "",
        "двадцать",
        "тридцать",
        "сорок",
        "пятьдесят",
        "шестьдесят",
        "семьдесят",
        "восемьдесят",
        "девяносто",
    ];
    let hundreds = [
        "",
        "сто",
        "двести",
        "триста",
        "четыреста",
        "пятьсот",
        "шестьсот",
        "семьсот",
        "восемьсот",
        "девятьсот",
    ];
    fn under_100(n: u64, ones_m: &[&str], teens: &[&str], tens: &[&str]) -> String {
        if n < 10 {
            ones_m[n as usize].into()
        } else if n < 20 {
            teens[(n - 10) as usize].into()
        } else {
            let t = tens[(n / 10) as usize];
            let o = n % 10;
            if o == 0 {
                t.into()
            } else {
                format!("{t} {}", ones_m[o as usize])
            }
        }
    }
    if n < 100 {
        return under_100(n, &ones_m, &teens, &tens);
    }
    if n < 1000 {
        let h = hundreds[(n / 100) as usize];
        let r = n % 100;
        if r == 0 {
            h.into()
        } else {
            format!("{h} {}", under_100(r, &ones_m, &teens, &tens))
        }
    } else if n < 1_000_000 {
        let th = n / 1000;
        let r = n % 1000;
        let head = match th {
            1 => "одна тысяча".to_string(),
            2 => "две тысячи".to_string(),
            3..=4 => format!("{} тысячи", ones_m[th as usize]),
            _ => format!("{} тысяч", under_100(th, &ones_m, &teens, &tens)),
        };
        if r == 0 {
            head
        } else {
            format!("{head} {}", ru_cardinal(r))
        }
    } else {
        n.to_string()
    }
}

/// Numbers→words only (locale); leave scripts intact for vocab-aware CTC.
pub fn locale_prepare_numbers(text: &str, locale: &str) -> String {
    replace_numbers(text, locale).0
}

/// Numbers→words + full transliterate (search / walkthrough paths).
pub fn locale_prepare(text: &str, locale: &str) -> String {
    transliterate(&locale_prepare_numbers(text, locale))
}

fn replace_numbers(text: &str, locale: &str) -> (String, Mapping) {
    let lang = locale_lang(locale);
    let profile = currency_for_locale(locale);
    let mut mapping = Mapping::new();
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        // ST: currency before bare numbers. Try leading then trailing symbol order
        // (books use both even when Intl prefers one).
        if let Some((consumed, words)) = try_leading_currency(&chars[i..], locale, profile) {
            let start = i;
            i += consumed;
            mapping.insert_map(start as i32, consumed as i32, words.chars().count() as i32);
            out.push_str(&words);
            continue;
        }
        if let Some((consumed, words)) = try_trailing_currency(&chars[i..], locale, profile) {
            let start = i;
            i += consumed;
            mapping.insert_map(start as i32, consumed as i32, words.chars().count() as i32);
            out.push_str(&words);
            continue;
        }
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            let num_str: String = chars[start..i].iter().collect();
            let n: u64 = num_str.parse().unwrap_or(0);
            let words = number_to_words(n, lang);
            mapping.insert_map(
                start as i32,
                (i - start) as i32,
                words.chars().count() as i32,
            );
            out.push_str(&words);
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    (out, mapping)
}

fn match_symbol_at(chars: &[char], symbols: &'static [&'static str]) -> Option<&'static str> {
    let s: String = chars.iter().collect();
    symbols.iter().find(|&&sym| s.starts_with(sym)).copied()
}

fn parse_currency_amount(chars: &[char], mut i: usize) -> Option<(usize, u64, u64)> {
    if i >= chars.len() || !chars[i].is_ascii_digit() {
        return None;
    }
    let num_start = i;
    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == ',') {
        i += 1;
    }
    let mut whole = String::new();
    let mut frac = String::new();
    let mut seen_dot = false;
    if i < chars.len()
        && (chars[i] == '.' || chars[i] == ',')
        && chars.get(i + 1).is_some_and(|c| c.is_ascii_digit())
    {
        seen_dot = true;
        i += 1;
        let frac_start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        for &c in &chars[num_start..frac_start - 1] {
            if c != ',' {
                whole.push(c);
            }
        }
        frac = chars[frac_start..i].iter().collect();
    }
    if !seen_dot {
        for &c in &chars[num_start..i] {
            if c != ',' {
                whole.push(c);
            }
        }
    }
    if whole.is_empty() {
        return None;
    }
    let dollars: u64 = whole.parse().ok()?;
    let cents: u64 = if frac.is_empty() {
        0
    } else {
        let padded = format!("{frac:0<2}");
        padded[..2.min(padded.len())].parse().unwrap_or(0)
    };
    Some((i, dollars, cents))
}

fn format_currency_words(
    dollars: u64,
    cents: u64,
    locale: &str,
    profile: CurrencyProfile,
) -> String {
    let lang = locale_lang(locale);
    let word_lang = match lang {
        "fr" | "ru" => lang,
        _ => "en",
    };
    let major = number_to_words(dollars, word_lang);
    let major_noun = if dollars == 1 || profile.invariant_major {
        profile.major.to_string()
    } else {
        profile.major_plural.to_string()
    };
    if cents == 0 {
        format!("{major} {major_noun}")
    } else {
        let minor = number_to_words(cents, word_lang);
        let minor_noun = if cents == 1 {
            profile.minor.to_string()
        } else {
            profile.minor_plural.to_string()
        };
        format!("{major} {major_noun} and {minor} {minor_noun}")
    }
}

/// Storyteller `toWords(..., { currency: true, doNotAddOnly: true })` for leading
/// currency symbols (maximize + locale-currency shaped profile).
fn try_leading_currency(
    chars: &[char],
    locale: &str,
    profile: CurrencyProfile,
) -> Option<(usize, String)> {
    let sym = match_symbol_at(chars, profile.symbols)?;
    let units = unit_for_symbol(sym, profile);
    let mut i = sym.chars().count();
    if chars.get(i).copied() == Some(' ') || chars.get(i).copied() == Some('\u{00A0}') {
        i += 1;
    }
    let (end, dollars, cents) = parse_currency_amount(chars, i)?;
    Some((end, format_currency_words(dollars, cents, locale, units)))
}

/// Trailing currency (`currencyLeading: false` Intl shape) for locale symbols.
fn try_trailing_currency(
    chars: &[char],
    locale: &str,
    profile: CurrencyProfile,
) -> Option<(usize, String)> {
    chars.first().filter(|c| c.is_ascii_digit())?;
    let (num_end, dollars, cents) = parse_currency_amount(chars, 0)?;
    let mut j = num_end;
    if chars.get(j).copied() == Some(' ') || chars.get(j).copied() == Some('\u{00A0}') {
        j += 1;
    }
    let sym = match_symbol_at(&chars[j..], profile.symbols)?;
    let units = unit_for_symbol(sym, profile);
    let end = j + sym.chars().count();
    Some((end, format_currency_words(dollars, cents, locale, units)))
}

/// Word entry for [`slugified_word_ranges`] (ST Echogarden-shaped offsets).
#[derive(Debug, Clone)]
pub struct WordEntry {
    pub start_offset: usize,
    pub end_offset: usize,
    pub is_punctuation: bool,
}

/// Whitespace-split words with Unicode-scalar offsets (CLDR segment residual).
pub fn word_entries_whitespace(sentence: &str) -> Vec<WordEntry> {
    let chars: Vec<char> = sentence.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        let token: String = chars[start..i].iter().collect();
        let is_punct = token.chars().all(|c| !c.is_alphanumeric());
        out.push(WordEntry {
            start_offset: start,
            end_offset: i,
            is_punctuation: is_punct,
        });
    }
    out
}

/// Storyteller `slugifiedWordRanges` — map word offsets through Mapping into inclusive slug spans.
pub fn slugified_word_ranges(
    words: &[WordEntry],
    mapping: &mut Mapping,
    sentence_start: usize,
    slug_start: usize,
) -> Vec<(usize, usize)> {
    let mut cursor = mapping.cursor();
    words
        .iter()
        .filter(|w| !w.is_punctuation)
        .map(|word| {
            let start = cursor.map(
                (word.start_offset as i32) - (sentence_start as i32),
                MapAssoc::Start,
            ) as usize;
            let end_raw = cursor.map(
                (word.end_offset as i32) - (sentence_start as i32),
                MapAssoc::End,
            ) as usize;
            let end = end_raw.saturating_sub(1).max(start);
            (start + slug_start, end + slug_start)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_punct_and_joins_with_dashes() {
        assert_eq!(slugify_for_search("Hello, Peter Pan!"), "hello-peter-pan");
    }

    #[test]
    fn opencc_folds_traditional_when_only_simplified_in_vocab() {
        let trad = '書';
        let simp = '书';
        let map = traditional_to_simplified();
        assert_eq!(map.get(&trad.to_string()).map(String::as_str), Some("书"));

        let mut vocab = HashMap::new();
        vocab.insert(simp.to_string(), 1);
        let folded = fold_han_for_vocab(&format!("hello{trad}world"), &vocab);
        assert!(folded.contains(simp));
        assert!(!folded.contains(trad));
    }

    #[test]
    fn opencc_keeps_traditional_when_in_vocab() {
        let trad = '書';
        let mut vocab = HashMap::new();
        vocab.insert(trad.to_string(), 1);
        vocab.insert("书".into(), 2);
        let folded = fold_han_for_vocab(&trad.to_string(), &vocab);
        assert_eq!(folded, trad.to_string());
    }

    #[test]
    fn transliterate_walkthrough_step2() {
        assert_eq!(
            transliterate("Hello there, it's a fine day! 42 cats."),
            "Hello there, it's a fine day! 42 cats."
        );
        assert_eq!(
            transliterate("Ça a été très difficile, l'hôtel était 42 fermé."),
            "Ca a ete tres difficile, l'hotel etait 42 ferme."
        );
        assert_eq!(
            transliterate("Съешь же ещё этих мягких французских 42 слова булок."),
            "Sesh zhe eshchyo etih myagkih francuzskih 42 slova bulok."
        );
    }

    #[test]
    fn slugify_dash_walkthrough_step3() {
        assert_eq!(
            slugify_for_search_with_locale("Hello there, it's a fine day! 42 cats.", "en"),
            "hello-there-it-s-a-fine-day-forty-two-cats"
        );
        assert_eq!(
            slugify_for_search_with_locale(
                "Ça a été très difficile, l'hôtel était 42 fermé.",
                "fr",
            ),
            "ca-a-ete-tres-difficile-l-hotel-etait-quarante-deux-ferme"
        );
        assert_eq!(
            slugify_for_search_with_locale(
                "Съешь же ещё этих мягких французских 42 слова булок.",
                "ru",
            ),
            "sesh-zhe-eshchyo-etih-myagkih-francuzskih-sorok-dva-slova-bulok"
        );
    }

    #[test]
    fn slugify_empty_sep_walkthrough_step3() {
        assert_eq!(
            slugify_joined("Hello there, it's a fine day! 42 cats.", "en"),
            "hellothereitsafinedayfortytwocats"
        );
        assert_eq!(
            slugify_joined("Ça a été très difficile, l'hôtel était 42 fermé.", "fr"),
            "caaetetresdifficilelhoteletaitquarantedeuxferme"
        );
        assert_eq!(
            slugify_joined("Съешь же ещё этих мягких французских 42 слова булок.", "ru"),
            "seshzheeshchyoetihmyagkihfrancuzskihsorokdvaslovabulok"
        );
    }

    #[test]
    fn mapping_ca_42_cats_empty_sep() {
        let text = "Ça, 42 cats!";
        let SlugifyOutcome {
            result,
            mut mapping,
        } = slugify_joined_mapped(text, "en");
        assert_eq!(result, "cafortytwocats");
        let mut cursor = mapping.cursor();
        assert_eq!(cursor.map(0, MapAssoc::Start), 0);
        assert_eq!(cursor.map(1, MapAssoc::Start), 1);
        let start42 = cursor.map(4, MapAssoc::Start);
        let end42 = cursor.map(6, MapAssoc::End);
        assert_eq!(&result[start42 as usize..end42 as usize], "fortytwo");
    }

    #[test]
    fn en_currency_leading_dollar_to_words() {
        assert_eq!(
            locale_prepare_numbers("It cost $42.", "en"),
            "It cost Forty Two dollars."
        );
        assert_eq!(
            locale_prepare_numbers("$1.50 please", "en"),
            "One dollar and Fifty cents please"
        );
        assert!(
            slugify_for_search_with_locale("Paid $42 for it", "en").contains("forty-two-dollars")
                || slugify_for_search_with_locale("Paid $42 for it", "en")
                    .contains("fortytwodollars")
                || slugify_for_search_with_locale("Paid $42 for it", "en").contains("forty-two")
        );
        let joined = slugify_joined("Paid $42 for it", "en");
        assert!(joined.contains("fortytwodollars") || joined.contains("fortytwo"));
    }

    #[test]
    fn fr_euro_leading_currency_to_words() {
        let out = locale_prepare_numbers("Ça coûte €42.", "fr");
        assert!(out.to_lowercase().contains("euro"), "{out}");
        assert!(!out.contains('€'), "{out}");
    }

    #[test]
    fn de_euro_trailing_currency_to_words() {
        let out = locale_prepare_numbers("Das kostet 42 €.", "de");
        assert!(out.to_lowercase().contains("euro"), "{out}");
        assert!(!out.contains('€'), "{out}");
    }

    #[test]
    fn ja_yen_leading_currency_to_words() {
        let out = locale_prepare_numbers("それは ¥42 です", "ja");
        assert!(out.to_lowercase().contains("yen"), "{out}");
        assert!(!out.contains('¥'), "{out}");
        assert!(!out.to_lowercase().contains("yens"), "{out}");
    }

    #[test]
    fn ko_won_leading_currency_to_words() {
        let out = locale_prepare_numbers("가격은 ₩42", "ko");
        assert!(out.to_lowercase().contains("won"), "{out}");
        assert!(!out.contains('₩'), "{out}");
    }

    #[test]
    fn zh_yen_leading_currency_to_words() {
        let out = locale_prepare_numbers("价格是 ¥42", "zh");
        assert!(out.to_lowercase().contains("yen"), "{out}");
        assert!(!out.contains('¥'), "{out}");
        assert!(!out.to_lowercase().contains("yens"), "{out}");
    }

    #[test]
    fn en_pound_and_euro_leading_currency_to_words() {
        let pound = locale_prepare_numbers("It cost £5.", "en");
        assert!(pound.to_lowercase().contains("pound"), "{pound}");
        assert!(!pound.contains('£'), "{pound}");
        let euro = locale_prepare_numbers("It cost €3.", "en");
        assert!(euro.to_lowercase().contains("euro"), "{euro}");
        assert!(!euro.contains('€'), "{euro}");
    }

    #[test]
    fn rarer_locale_currency_maximize_to_words() {
        let sv = locale_prepare_numbers("Det kostar 42 kr.", "sv");
        assert!(sv.to_lowercase().contains("kron"), "{sv}");
        assert!(!sv.contains(" 42 kr"), "{sv}");

        let pl = locale_prepare_numbers("Kosztuje 42 zł.", "pl");
        assert!(
            pl.to_lowercase().contains("zloty") || pl.to_lowercase().contains("zlotys"),
            "{pl}"
        );

        let cs = locale_prepare_numbers("Stojí 42 Kč.", "cs");
        assert!(
            cs.to_lowercase().contains("koruna") || cs.to_lowercase().contains("korunas"),
            "{cs}"
        );

        let ru = locale_prepare_numbers("Это ₽42", "ru");
        assert!(ru.to_lowercase().contains("ruble"), "{ru}");
        assert!(!ru.contains('₽'), "{ru}");

        let br = locale_prepare_numbers("Custa R$42.", "pt-BR");
        assert!(br.to_lowercase().contains("real"), "{br}");

        let hi = locale_prepare_numbers("कीमत ₹42", "hi");
        assert!(hi.to_lowercase().contains("rupee"), "{hi}");

        let tr = locale_prepare_numbers("Fiyat ₺42", "tr");
        assert!(tr.to_lowercase().contains("lira"), "{tr}");

        let th = locale_prepare_numbers("ราคา ฿42", "th");
        assert!(th.to_lowercase().contains("baht"), "{th}");
        assert!(!th.to_lowercase().contains("bahts"), "{th}");

        let hu = locale_prepare_numbers("Ára 42 Ft", "hu");
        assert!(hu.to_lowercase().contains("forint"), "{hu}");

        let ua = locale_prepare_numbers("Ціна ₴42", "uk");
        assert!(ua.to_lowercase().contains("hryvnia"), "{ua}");
    }

    #[test]
    fn slugified_word_ranges_maps_number_expansion() {
        let text = "The 1 was next";
        let SlugifyOutcome {
            result,
            mut mapping,
        } = slugify_joined_mapped(text, "en");
        let words = word_entries_whitespace(text);
        let ranges = slugified_word_ranges(&words, &mut mapping, 0, 0);
        assert_eq!(ranges.len(), 4);
        assert_eq!(&result[ranges[0].0..=ranges[0].1], "the");
        assert_eq!(&result[ranges[1].0..=ranges[1].1], "one");
        assert_eq!(&result[ranges[2].0..=ranges[2].1], "was");
        assert_eq!(&result[ranges[3].0..=ranges[3].1], "next");
    }
}
