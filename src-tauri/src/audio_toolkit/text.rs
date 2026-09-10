use natural::phonetics::soundex;
use once_cell::sync::Lazy;
use regex::Regex;
use strsim::levenshtein;

/// Builds an n-gram string by cleaning and concatenating words
///
/// Strips punctuation from each word, lowercases, and joins without spaces.
/// This allows matching "Charge B" against "ChargeBee".
fn build_ngram(words: &[&str]) -> String {
    words
        .iter()
        .map(|w| build_match_key(w))
        .collect::<Vec<_>>()
        .concat()
}

fn build_match_key(word: &str) -> String {
    word.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

struct CustomWordMatchKey {
    word_index: usize,
    key: String,
}

fn build_custom_word_match_keys(word: &str, word_index: usize) -> Vec<CustomWordMatchKey> {
    let primary_key = build_match_key(word);
    let mut keys = Vec::with_capacity(2);

    // The fallback matcher is intentionally limited to ASCII terms. Its
    // whitespace tokenization and Soundex scoring are not suitable for CJK
    // scripts. Unicode custom words remain available to models that accept
    // them as native decode prompts; they are simply skipped by this fallback.
    if is_supported_fuzzy_key(&primary_key) {
        keys.push(CustomWordMatchKey {
            word_index,
            key: primary_key.clone(),
        });
    }

    if word.contains('&') {
        let expanded_key = build_match_key(&word.replace('&', " and "));
        if is_supported_fuzzy_key(&expanded_key) && expanded_key != primary_key {
            keys.push(CustomWordMatchKey {
                word_index,
                key: expanded_key,
            });
        }
    }

    keys
}

fn is_supported_fuzzy_key(key: &str) -> bool {
    !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric())
}

fn supports_soundex(key: &str) -> bool {
    !key.is_empty() && key.chars().all(|c| c.is_ascii_alphabetic())
}

/// Finds the best matching custom word for a candidate string
///
/// Uses Levenshtein distance and Soundex phonetic matching to find
/// the best match above the given threshold.
///
/// # Arguments
/// * `candidate` - The cleaned/lowercased candidate string to match
/// * `custom_words` - Original custom words (for returning the replacement)
/// * `custom_word_match_keys` - Normalized custom-word keys for comparison
/// * `threshold` - Maximum similarity score to accept
///
/// # Returns
/// The best matching custom word and its score, if any match was found
fn find_best_match<'a>(
    candidate: &str,
    custom_words: &'a [String],
    custom_word_match_keys: &[CustomWordMatchKey],
    threshold: f64,
) -> Option<(&'a String, f64)> {
    if !is_supported_fuzzy_key(candidate) || candidate.chars().count() > 50 {
        return None;
    }

    let mut best_match: Option<&String> = None;
    let mut best_score = f64::MAX;

    for custom_word_key in custom_word_match_keys {
        // Skip if lengths are too different (optimization + prevents over-matching)
        // Use percentage-based check: max 25% length difference (prevents n-grams from
        // matching significantly shorter custom words, e.g., "openaigpt" vs "openai")
        let candidate_len = candidate.chars().count();
        let custom_word_len = custom_word_key.key.chars().count();
        let len_diff = candidate_len.abs_diff(custom_word_len) as f64;
        let max_len = candidate_len.max(custom_word_len) as f64;
        let max_allowed_diff = (max_len * 0.25).max(2.0); // At least 2 chars difference allowed
        if len_diff > max_allowed_diff {
            continue;
        }

        // Calculate Levenshtein distance (normalized by length)
        let levenshtein_dist = levenshtein(candidate, &custom_word_key.key);
        let levenshtein_score = if max_len > 0.0 {
            levenshtein_dist as f64 / max_len
        } else {
            1.0
        };

        // Soundex is an English/ASCII phonetic algorithm. Numeric terms can
        // still use edit distance, but must not receive a phonetic boost.
        let phonetic_match = supports_soundex(candidate)
            && supports_soundex(&custom_word_key.key)
            && soundex(candidate, &custom_word_key.key);

        // Combine scores: favor phonetic matches, but also consider string similarity
        let combined_score = if phonetic_match {
            levenshtein_score * 0.3 // Give significant boost to phonetic matches
        } else {
            levenshtein_score
        };

        // Accept if the score is good enough (configurable threshold)
        if combined_score < threshold && combined_score < best_score {
            best_match = Some(&custom_words[custom_word_key.word_index]);
            best_score = combined_score;
        }
    }

    best_match.map(|m| (m, best_score))
}

/// Applies custom word corrections to transcribed text using fuzzy matching
///
/// This function corrects words in the input text by finding the best matches
/// from a list of custom words using a combination of:
/// - Levenshtein distance for string similarity
/// - Soundex phonetic matching for pronunciation similarity
/// - N-gram matching for multi-word speech artifacts (e.g., "Charge B" -> "ChargeBee")
///
/// # Arguments
/// * `text` - The input text to correct
/// * `custom_words` - List of custom words to match against
/// * `threshold` - Maximum similarity score to accept (0.0 = exact match, 1.0 = any match)
///
/// # Returns
/// The corrected text with custom words applied
pub fn apply_custom_words(text: &str, custom_words: &[String], threshold: f64) -> String {
    if custom_words.is_empty() {
        return text.to_string();
    }

    // Pre-compute normalized comparison keys to avoid repeated allocations.
    let custom_word_match_keys: Vec<CustomWordMatchKey> = custom_words
        .iter()
        .enumerate()
        .flat_map(|(index, word)| build_custom_word_match_keys(word, index))
        .collect();

    let words: Vec<&str> = text.split_whitespace().collect();
    let mut result = Vec::new();
    let mut i = 0;

    while i < words.len() {
        let mut best_match: Option<(usize, &String, f64)> = None;

        // Consider n-grams up to three words and choose the closest match. A
        // longest-first match can consume a following ordinary word when both
        // candidates happen to share a Soundex code (for example,
        // "Charge B, che" matching "ChargeBee").
        for n in (1..=3).rev() {
            if i + n > words.len() {
                continue;
            }

            let ngram_words = &words[i..i + n];
            // Do not consume across a punctuation boundary. In
            // "Charge B, che", the comma closes the candidate at "B,".
            if ngram_words[..n.saturating_sub(1)]
                .iter()
                .any(|word| !extract_punctuation(word).1.is_empty())
            {
                continue;
            }
            let ngram = build_ngram(ngram_words);

            if let Some((replacement, score)) =
                find_best_match(&ngram, custom_words, &custom_word_match_keys, threshold)
            {
                let is_better = best_match
                    .as_ref()
                    .is_none_or(|(_, _, best_score)| score < *best_score);
                if is_better {
                    best_match = Some((n, replacement, score));
                }
            }
        }

        if let Some((n, replacement, _)) = best_match {
            let ngram_words = &words[i..i + n];
            // Extract punctuation from first and last words of the n-gram.
            let (prefix, _) = extract_punctuation(ngram_words[0]);
            let (_, suffix) = extract_punctuation(ngram_words[n - 1]);

            // Preserve case from first word.
            let corrected = preserve_case_pattern(ngram_words[0], replacement);

            result.push(format!("{}{}{}", prefix, corrected, suffix));
            i += n;
        } else {
            result.push(words[i].to_string());
            i += 1;
        }
    }

    result.join(" ")
}

/// Preserves the case pattern of the original word when applying a replacement
fn preserve_case_pattern(original: &str, replacement: &str) -> String {
    if original.chars().all(|c| c.is_uppercase()) {
        replacement.to_uppercase()
    } else if original.chars().next().is_some_and(|c| c.is_uppercase()) {
        let mut chars: Vec<char> = replacement.chars().collect();
        if let Some(first_char) = chars.get_mut(0) {
            *first_char = first_char.to_uppercase().next().unwrap_or(*first_char);
        }
        chars.into_iter().collect()
    } else {
        replacement.to_string()
    }
}

/// Extracts punctuation prefix and suffix from a word
fn extract_punctuation(word: &str) -> (&str, &str) {
    // String slices use byte offsets. Derive both boundaries from char_indices
    // so multibyte punctuation such as `。` and `「」` can never be split.
    let prefix_end = word
        .char_indices()
        .find(|(_, c)| c.is_alphanumeric())
        .map(|(index, _)| index)
        .unwrap_or(word.len());
    let suffix_start = word
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_alphanumeric())
        .map(|(index, c)| index + c.len_utf8())
        .unwrap_or(0);

    let prefix = if prefix_end > 0 {
        &word[..prefix_end]
    } else {
        ""
    };

    let suffix = if suffix_start < word.len() {
        &word[suffix_start..]
    } else {
        ""
    };

    (prefix, suffix)
}

/// Evidence for the language of the text being cleaned.
///
/// This intentionally describes the transcription output, not Handy's UI
/// language. Unknown output languages fail closed: built-in filler removal is
/// skipped rather than applying a language profile speculatively.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputLanguageEvidence {
    UserSelected(String),
    ModelConstrained(String),
    /// The transcription model itself identified the language (audio-based
    /// LID, e.g. Whisper in auto mode).
    ModelDetected(String),
    /// Detected from the transcribed text with high confidence, constrained to
    /// the model's supported languages. Weakest accepted evidence.
    TextDetected(String),
    TranslatedToEnglish,
    Unknown,
}

impl OutputLanguageEvidence {
    fn language(&self) -> Option<&str> {
        match self {
            Self::UserSelected(language)
            | Self::ModelConstrained(language)
            | Self::ModelDetected(language)
            | Self::TextDetected(language) => Some(language),
            Self::TranslatedToEnglish => Some("en"),
            Self::Unknown => None,
        }
    }
}

/// Filler tokens that are not lexical words in any language Handy's models can
/// output, so removing them cannot corrupt text regardless of the (possibly
/// unknown) output language. Kept deliberately conservative: anything that is a
/// real word somewhere ("um" pt/de, "ha" es, "ah"/"eh" interjections, "mm"
/// millimetres) belongs in the language-gated lists instead.
const UNIVERSAL_FILLER_WORDS: &[&str] = &[
    "uh", "uhm", "umm", "uhh", "uhhh", "ehh", "ehm", "ahm", "hmm", "hm", "mmm", "хм", "ммм",
];

/// Filler words that are only safe to remove with evidence for the output
/// language, because the same token is a real word elsewhere (e.g. Portuguese
/// "um" = "a/an", German "um" = "at/around", Spanish "ha" = "has").
fn gated_filler_words_for_language(lang: &str) -> &'static [&'static str] {
    let base_lang = lang.split(&['-', '_'][..]).next().unwrap_or(lang);

    match base_lang {
        "en" => &["um", "ah", "eh", "ha"],
        "de" => &["äh", "ähm"],
        "fr" => &["euh"],
        _ => &[],
    }
}

static MULTI_SPACE_PATTERN: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s{2,}").unwrap());

/// Collapses repeated words (3+ repetitions) to a single instance.
/// E.g., "wh wh wh wh" -> "wh", "I I I I" -> "I"
fn collapse_stutters(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return text.to_string();
    }

    let mut result: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < words.len() {
        let word = words[i];
        let word_lower = word.to_lowercase();

        if word_lower.chars().all(|c| c.is_alphabetic()) {
            // Count consecutive repetitions (case-insensitive)
            let mut count = 1;
            while i + count < words.len() && words[i + count].to_lowercase() == word_lower {
                count += 1;
            }

            // If 3+ repetitions, collapse to single instance
            if count >= 3 {
                result.push(word);
                i += count;
            } else {
                result.push(word);
                i += 1;
            }
        } else {
            result.push(word);
            i += 1;
        }
    }

    result.join(" ")
}

/// Removes filler words from transcription output when enabled.
///
/// Built-in removal is two-tiered: [`UNIVERSAL_FILLER_WORDS`] apply regardless
/// of language evidence, while [`gated_filler_words_for_language`] tokens are
/// only removed when the output language is known. A custom list is an
/// explicit user override and replaces both tiers without requiring language
/// evidence. `Some(empty vec)` disables removal, preserving the legacy
/// power-user setting. The master toggle takes precedence over both built-in
/// and custom lists.
///
/// # Arguments
/// * `text` - The raw transcription text to filter
/// * `language` - Evidence for the language of the transcription output
/// * `custom_filler_words` - Optional user-provided filler word list. `Some(vec)` overrides
///   language defaults; `Some(empty vec)` disables filtering; `None` uses language defaults.
/// * `enabled` - Whether filler-word removal is enabled
///
/// # Returns
/// The text with configured filler words removed
pub fn remove_filler_words(
    text: &str,
    language: &OutputLanguageEvidence,
    custom_filler_words: &Option<Vec<String>>,
    enabled: bool,
) -> String {
    if !enabled {
        return text.to_string();
    }

    // Build filler patterns from custom list or the built-in tiers
    let patterns: Vec<Regex> = match custom_filler_words {
        Some(words) => words
            .iter()
            .filter_map(|word| Regex::new(&format!(r"(?i)\b{}\b[,.]?", regex::escape(word))).ok())
            .collect(),
        None => UNIVERSAL_FILLER_WORDS
            .iter()
            .chain(
                language
                    .language()
                    .map(gated_filler_words_for_language)
                    .unwrap_or_default(),
            )
            .map(|word| Regex::new(&format!(r"(?i)\b{}\b[,.]?", regex::escape(word))).unwrap())
            .collect(),
    };

    // Remove filler words
    let mut filtered = text.to_string();
    for pattern in &patterns {
        filtered = pattern.replace_all(&filtered, "").to_string();
    }

    filtered
}

/// Applies non-filler transcription cleanup.
///
/// Kept separate from [`remove_filler_words`] so disabling filler deletion
/// does not also disable the existing repeated-word and whitespace cleanup.
pub fn normalize_transcription_output(text: &str) -> String {
    let mut normalized = collapse_stutters(text);

    // Clean up multiple spaces to single space
    normalized = MULTI_SPACE_PATTERN
        .replace_all(&normalized, " ")
        .to_string();

    // Trim leading/trailing whitespace
    normalized.trim().to_string()
}

/// Symbol produced by a spoken dictation command such as "virgule" or
/// "à la ligne".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpokenSymbol {
    Comma,
    Period,
    Ellipsis,
    QuestionMark,
    ExclamationMark,
    Colon,
    Semicolon,
    NewLine,
    NewParagraph,
    OpenParenthesis,
    CloseParenthesis,
    OpenQuote,
    CloseQuote,
}

struct SpokenCommand {
    /// Regex fragment without capturing groups, matched case-insensitively
    /// between word boundaries.
    pattern: &'static str,
    symbol: SpokenSymbol,
    /// The phrase is also an ordinary French expression ("le point", "une
    /// nouvelle ligne de bus"). It is only converted when the output language
    /// is French or unknown and the neighbouring words do not look like
    /// ordinary usage.
    ambiguous: bool,
}

const fn spoken(pattern: &'static str, symbol: SpokenSymbol, ambiguous: bool) -> SpokenCommand {
    SpokenCommand {
        pattern,
        symbol,
        ambiguous,
    }
}

/// French dictation commands. Order matters: the combined regex uses
/// leftmost-first alternation, so longer phrases must precede their prefixes
/// ("point d'interrogation" before "point").
const FRENCH_SPOKEN_COMMANDS: &[SpokenCommand] = &[
    spoken(r"retour\s+à\s+la\s+ligne", SpokenSymbol::NewLine, false),
    spoken(r"à\s+la\s+ligne", SpokenSymbol::NewLine, false),
    spoken(r"saut\s+de\s+ligne", SpokenSymbol::NewLine, false),
    spoken(r"nouvelle\s+ligne", SpokenSymbol::NewLine, true),
    spoken(r"nouveau\s+paragraphe", SpokenSymbol::NewParagraph, true),
    spoken(r"point[\s-]+d['’]\s*interrogation", SpokenSymbol::QuestionMark, false),
    spoken(r"point[\s-]+d['’]\s*exclamation", SpokenSymbol::ExclamationMark, false),
    spoken(r"point[\s-]+virgule", SpokenSymbol::Semicolon, false),
    spoken(r"points\s+de\s+suspension", SpokenSymbol::Ellipsis, false),
    spoken(r"trois\s+petits\s+points", SpokenSymbol::Ellipsis, false),
    spoken(r"point\s+final", SpokenSymbol::Period, true),
    spoken(r"deux[\s-]+points", SpokenSymbol::Colon, true),
    spoken(r"point", SpokenSymbol::Period, true),
    spoken(r"virgule", SpokenSymbol::Comma, false),
    spoken(r"ouvr(?:ez|ir|e)\s+(?:la\s+)?parenth[èe]se", SpokenSymbol::OpenParenthesis, false),
    spoken(r"ferm(?:ez|er|e)\s+(?:la\s+)?parenth[èe]se", SpokenSymbol::CloseParenthesis, false),
    spoken(r"ouvr(?:ez|ir|e)\s+(?:les\s+)?guillemets?", SpokenSymbol::OpenQuote, false),
    spoken(r"ferm(?:ez|er|e)\s+(?:les\s+)?guillemets?", SpokenSymbol::CloseQuote, false),
];

/// One capturing group per command, in table order, so the matching group
/// index identifies the command.
static FRENCH_SPOKEN_COMMAND_PATTERN: Lazy<Regex> = Lazy::new(|| {
    let alternatives: Vec<String> = FRENCH_SPOKEN_COMMANDS
        .iter()
        .map(|command| format!("({})", command.pattern))
        .collect();
    Regex::new(&format!(r"(?i)\b(?:{})\b", alternatives.join("|"))).unwrap()
});

/// Words that, right before an ambiguous command, show it is an ordinary noun
/// ("le point", "une nouvelle ligne", "the point").
const ORDINARY_USAGE_PRECEDING_WORDS: &[&str] = &[
    "le", "la", "les", "l'", "un", "une", "des", "du", "de", "d'", "au", "aux", "à", "ce", "cet",
    "cette", "ces", "mon", "ma", "mes", "ton", "ta", "tes", "son", "sa", "ses", "notre", "nos",
    "votre", "vos", "leur", "leurs", "quel", "quelle", "quels", "quelles", "chaque", "même",
    "bon", "bonne", "premier", "première", "dernier", "dernière", "seul", "seule", "tel",
    "telle", "en", "sur", "par", "quelques", "plusieurs", "the", "a", "an", "this", "that", "my",
    "your", "his", "her", "our", "their", "no", "any", "some", "what", "whole", "main", "good",
];

/// Words that, right after an ambiguous command, show it is an ordinary noun
/// ("point de vue", "deux points à voir", "gmail point com").
const ORDINARY_USAGE_FOLLOWING_WORDS: &[&str] = &[
    "de", "du", "des", "à", "au", "aux", "où", "en", "sur", "pour", "par", "qui", "que", "com",
    "fr", "net", "org", "commun", "communs", "fort", "forts", "faible", "faibles", "important",
    "importants", "importante", "importantes", "essentiel", "essentiels", "clé", "clés",
    "principal", "principaux", "précis", "noir", "noirs", "mort", "positif", "positifs",
    "négatif", "négatifs", "culminant", "central", "chaud", "chauds", "is", "of", "was", "in",
    "that",
];

fn is_dictation_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '\'' | '’' | '-')
}

fn normalize_dictation_word(word: &str) -> String {
    word.to_lowercase().replace('’', "'")
}

/// Returns the word directly before `start`, or `None` when the command is
/// separated from it by punctuation, which marks it as a standalone command.
fn word_before(text: &str, start: usize) -> Option<String> {
    let before = text[..start].trim_end();
    if !before.chars().next_back().is_some_and(is_dictation_word_char) {
        return None;
    }
    let word_start = before
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_dictation_word_char(*c))
        .last()
        .map(|(index, _)| index)
        .unwrap_or(0);
    let word = normalize_dictation_word(&before[word_start..]);

    // "jusqu'au point" -> "au", "d'un point" -> "un"
    Some(match word.rsplit_once('\'') {
        Some((_, tail)) if !tail.is_empty() => tail.to_string(),
        _ => word,
    })
}

/// Returns the word directly after `end`, or `None` when punctuation follows.
fn word_after(text: &str, end: usize) -> Option<String> {
    let after = text[end..].trim_start();
    let word_end = after
        .char_indices()
        .find(|(_, c)| !is_dictation_word_char(*c))
        .map(|(index, _)| index)
        .unwrap_or(after.len());
    (word_end > 0).then(|| normalize_dictation_word(&after[..word_end]))
}

fn looks_like_ordinary_usage(text: &str, start: usize, end: usize) -> bool {
    let preceded = word_before(text, start)
        .is_some_and(|word| ORDINARY_USAGE_PRECEDING_WORDS.contains(&word.as_str()));
    let followed = word_after(text, end).is_some_and(|word| {
        word.starts_with("d'")
            || word.starts_with("qu'")
            || ORDINARY_USAGE_FOLLOWING_WORDS.contains(&word.as_str())
    });
    preceded || followed
}

/// `\b` treats apostrophes and hyphens as boundaries; a command glued to one
/// is part of a larger word ("Point-à-Pitre").
fn is_joined_to_word(text: &str, start: usize, end: usize) -> bool {
    let is_joiner = |c: char| matches!(c, '\'' | '’' | '-');
    text[..start].chars().next_back().is_some_and(is_joiner)
        || text[end..].chars().next().is_some_and(is_joiner)
}

/// Punctuation the speech model guessed around a spoken command. It is dropped
/// so the dictated symbol wins ("Bonjour, virgule," -> "Bonjour,").
fn is_model_punctuation(c: char) -> bool {
    matches!(c, ',' | '.' | ';' | ':' | '!' | '?' | '…')
}

fn trim_leading_model_punctuation(segment: &str) -> &str {
    segment.trim_start_matches(|c: char| c.is_whitespace() || is_model_punctuation(c))
}

/// Trims the text preceding a command. Model punctuation right before a
/// dictated mark is dropped; before a line break it is kept, since it usually
/// closes the sentence ("Merci. À la ligne" -> "Merci.\n").
fn trim_before_spoken_symbol(segment: &str, symbol: SpokenSymbol) -> &str {
    let segment = segment.trim_end();
    match symbol {
        SpokenSymbol::NewLine | SpokenSymbol::NewParagraph => segment,
        SpokenSymbol::OpenParenthesis | SpokenSymbol::OpenQuote => {
            segment.trim_end_matches(|c: char| c == ',' || c.is_whitespace())
        }
        SpokenSymbol::CloseParenthesis | SpokenSymbol::CloseQuote => segment
            .trim_end_matches(|c: char| matches!(c, ',' | ';' | ':' | '.') || c.is_whitespace()),
        _ => segment.trim_end_matches(|c: char| c.is_whitespace() || is_model_punctuation(c)),
    }
}

fn trim_trailing_spaces(output: &mut String) {
    let trimmed_len = output.trim_end_matches(&[' ', '\t'][..]).len();
    output.truncate(trimmed_len);
}

fn push_dictated_segment(
    output: &mut String,
    segment: &str,
    previous: Option<SpokenSymbol>,
    capitalize_next: &mut bool,
) {
    if segment.is_empty() {
        return;
    }

    let joins_directly = matches!(
        previous,
        None | Some(
            SpokenSymbol::NewLine
                | SpokenSymbol::NewParagraph
                | SpokenSymbol::OpenParenthesis
                | SpokenSymbol::OpenQuote
        )
    );
    if !joins_directly && !output.ends_with(char::is_whitespace) {
        output.push(' ');
    }

    let mut chars = segment.chars();
    if std::mem::take(capitalize_next) {
        if let Some(first) = chars.next() {
            output.extend(first.to_uppercase());
        }
    }
    output.push_str(chars.as_str());
}

fn push_spoken_symbol(output: &mut String, symbol: SpokenSymbol, capitalize_next: &mut bool) {
    match symbol {
        SpokenSymbol::Comma
        | SpokenSymbol::Period
        | SpokenSymbol::Ellipsis
        | SpokenSymbol::CloseParenthesis => {
            trim_trailing_spaces(output);
            output.push_str(match symbol {
                SpokenSymbol::Comma => ",",
                SpokenSymbol::Period => ".",
                SpokenSymbol::Ellipsis => "...",
                _ => ")",
            });
        }
        // French typography puts a space before two-part punctuation.
        SpokenSymbol::QuestionMark
        | SpokenSymbol::ExclamationMark
        | SpokenSymbol::Colon
        | SpokenSymbol::Semicolon => {
            trim_trailing_spaces(output);
            if output.ends_with(|c: char| !c.is_whitespace() && !matches!(c, '?' | '!' | '(')) {
                output.push(' ');
            }
            output.push(match symbol {
                SpokenSymbol::QuestionMark => '?',
                SpokenSymbol::ExclamationMark => '!',
                SpokenSymbol::Colon => ':',
                _ => ';',
            });
        }
        SpokenSymbol::NewLine | SpokenSymbol::NewParagraph => {
            trim_trailing_spaces(output);
            output.push_str(if symbol == SpokenSymbol::NewLine {
                "\n"
            } else {
                "\n\n"
            });
        }
        SpokenSymbol::OpenParenthesis | SpokenSymbol::OpenQuote => {
            if output.ends_with(|c: char| !c.is_whitespace() && !matches!(c, '(' | '«')) {
                output.push(' ');
            }
            output.push_str(if symbol == SpokenSymbol::OpenParenthesis {
                "("
            } else {
                "« "
            });
        }
        SpokenSymbol::CloseQuote => {
            trim_trailing_spaces(output);
            output.push_str(" »");
        }
    }

    *capitalize_next = match symbol {
        SpokenSymbol::Period
        | SpokenSymbol::QuestionMark
        | SpokenSymbol::ExclamationMark
        | SpokenSymbol::NewLine
        | SpokenSymbol::NewParagraph => true,
        // "à la ligne, ouvrez les guillemets, bonjour" -> "\n« Bonjour"
        SpokenSymbol::OpenParenthesis | SpokenSymbol::OpenQuote => *capitalize_next,
        _ => false,
    };
}

/// Converts spoken French punctuation and layout commands into the symbols
/// themselves: "Bonjour virgule à la ligne" becomes "Bonjour,\n".
///
/// Must run after [`normalize_transcription_output`], which collapses
/// whitespace and would erase the inserted line breaks. Text between commands
/// is kept verbatim; only punctuation the model guessed around a command is
/// dropped, so the dictated symbol wins.
///
/// # Arguments
/// * `text` - The normalized transcription text
/// * `language` - Evidence for the output language. Ambiguous commands such as
///   "point" are left alone when the output is known not to be French.
/// * `enabled` - Whether spoken punctuation commands are enabled
///
/// # Returns
/// The text with spoken commands replaced by punctuation and line breaks
pub fn apply_spoken_punctuation(
    text: &str,
    language: &OutputLanguageEvidence,
    enabled: bool,
) -> String {
    if !enabled {
        return text.to_string();
    }

    let allow_ambiguous = language
        .language()
        .is_none_or(|lang| lang.split(&['-', '_'][..]).next() == Some("fr"));

    let commands: Vec<(usize, usize, SpokenSymbol)> = FRENCH_SPOKEN_COMMAND_PATTERN
        .captures_iter(text)
        .filter_map(|captures| {
            let whole = captures.get(0)?;
            let command = FRENCH_SPOKEN_COMMANDS
                .iter()
                .enumerate()
                .find(|(index, _)| captures.get(index + 1).is_some())
                .map(|(_, command)| command)?;
            let (start, end) = (whole.start(), whole.end());
            if is_joined_to_word(text, start, end) {
                return None;
            }
            if command.ambiguous
                && (!allow_ambiguous || looks_like_ordinary_usage(text, start, end))
            {
                return None;
            }
            Some((start, end, command.symbol))
        })
        .collect();

    if commands.is_empty() {
        return text.to_string();
    }

    let mut output = String::with_capacity(text.len());
    let mut capitalize_next = false;
    let mut previous: Option<SpokenSymbol> = None;
    let mut cursor = 0;

    for (start, end, symbol) in commands {
        let mut segment = &text[cursor..start];
        if previous.is_some() {
            segment = trim_leading_model_punctuation(segment);
        }
        segment = trim_before_spoken_symbol(segment, symbol);
        push_dictated_segment(&mut output, segment, previous, &mut capitalize_next);
        push_spoken_symbol(&mut output, symbol, &mut capitalize_next);
        previous = Some(symbol);
        cursor = end;
    }

    let tail = trim_leading_model_punctuation(&text[cursor..]);
    push_dictated_segment(&mut output, tail, previous, &mut capitalize_next);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercise the complete cleanup sequence with an explicitly selected
    /// language. Individual tests below predate the split between filler
    /// removal and non-filler normalization.
    fn filter_transcription_output(
        text: &str,
        language: &str,
        custom_filler_words: &Option<Vec<String>>,
    ) -> String {
        let language = OutputLanguageEvidence::UserSelected(language.to_string());
        let filtered = remove_filler_words(text, &language, custom_filler_words, true);
        normalize_transcription_output(&filtered)
    }

    #[test]
    fn test_apply_custom_words_exact_match() {
        let text = "hello world";
        let custom_words = vec!["Hello".to_string(), "World".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert_eq!(result, "Hello World");
    }

    #[test]
    fn test_apply_custom_words_fuzzy_match() {
        let text = "helo wrold";
        let custom_words = vec!["hello".to_string(), "world".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_preserve_case_pattern() {
        assert_eq!(preserve_case_pattern("HELLO", "world"), "WORLD");
        assert_eq!(preserve_case_pattern("Hello", "world"), "World");
        assert_eq!(preserve_case_pattern("hello", "WORLD"), "WORLD");
    }

    #[test]
    fn test_extract_punctuation() {
        assert_eq!(extract_punctuation("hello"), ("", ""));
        assert_eq!(extract_punctuation("!hello?"), ("!", "?"));
        assert_eq!(extract_punctuation("...hello..."), ("...", "..."));
    }

    #[test]
    fn test_extract_punctuation_uses_unicode_boundaries() {
        assert_eq!(extract_punctuation("你好。"), ("", "。"));
        assert_eq!(extract_punctuation("「你好」"), ("「", "」"));
        assert_eq!(extract_punctuation("你好！"), ("", "！"));
    }

    #[test]
    fn test_empty_custom_words() {
        let text = "hello world";
        let custom_words = vec![];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_filter_filler_words() {
        let text = "So uhm I was thinking uh about this";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "So I was thinking about this");
    }

    #[test]
    fn test_filter_filler_words_case_insensitive() {
        let text = "UHM this is UH a test";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "this is a test");
    }

    #[test]
    fn test_filter_filler_words_with_punctuation() {
        let text = "Well, uhm, I think, uh. that's right";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "Well, I think, that's right");
    }

    #[test]
    fn test_filter_cleans_whitespace() {
        let text = "Hello    world   test";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "Hello world test");
    }

    #[test]
    fn test_filter_trims() {
        let text = "  Hello world  ";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "Hello world");
    }

    #[test]
    fn test_filter_combined() {
        let text = "  Uhm, so I was, uh, thinking about this  ";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "so I was, thinking about this");
    }

    #[test]
    fn test_filter_preserves_valid_text() {
        let text = "This is a completely normal sentence.";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "This is a completely normal sentence.");
    }

    #[test]
    fn test_filter_stutter_collapse() {
        let text = "w wh wh wh wh wh wh wh wh wh why";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "w wh why");
    }

    #[test]
    fn test_filter_stutter_short_words() {
        let text = "I I I I think so so so so";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "I think so");
    }

    #[test]
    fn test_filter_stutter_longer_words() {
        let text = "Check data doc doc doc doc documentation.";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "Check data doc documentation.");
    }

    #[test]
    fn test_filter_stutter_mixed_case() {
        let text = "No NO no NO no";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "No");
    }

    #[test]
    fn test_filter_stutter_preserves_two_repetitions() {
        let text = "no no is fine";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "no no is fine");
    }

    #[test]
    fn test_filter_english_removes_um() {
        let text = "um I think um this is good";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "I think this is good");
    }

    #[test]
    fn test_filter_portuguese_preserves_um() {
        // "um" means "a/an" in Portuguese
        let text = "um gato bonito";
        let result = filter_transcription_output(text, "pt", &None);
        assert_eq!(result, "um gato bonito");
    }

    #[test]
    fn test_filter_spanish_preserves_ha() {
        // "ha" means "has" in Spanish
        let text = "ha sido un buen día";
        let result = filter_transcription_output(text, "es", &None);
        assert_eq!(result, "ha sido un buen día");
    }

    #[test]
    fn test_filter_language_code_with_region() {
        // "pt-BR" should normalize to "pt"
        let text = "um gato bonito";
        let result = filter_transcription_output(text, "pt-BR", &None);
        assert_eq!(result, "um gato bonito");
    }

    #[test]
    fn test_filter_custom_filler_words_override() {
        let custom = Some(vec!["okay".to_string(), "right".to_string()]);
        let text = "okay so I think right this works";
        let result = filter_transcription_output(text, "en", &custom);
        assert_eq!(result, "so I think this works");
    }

    #[test]
    fn test_filter_custom_filler_words_empty_disables() {
        let custom = Some(vec![]);
        let text = "So uhm I was thinking uh about this";
        let result = filter_transcription_output(text, "en", &custom);
        // No filler words removed since custom list is empty
        assert_eq!(result, "So uhm I was thinking uh about this");
    }

    #[test]
    fn test_filter_unknown_language_still_removes_universal_fillers() {
        let text = "uh I think uhm this works";
        let result = filter_transcription_output(text, "xx", &None);
        assert_eq!(result, "I think this works");
    }

    #[test]
    fn test_filter_unknown_language_does_not_remove_um() {
        let text = "um I think this works";
        let result = filter_transcription_output(text, "xx", &None);
        assert_eq!(result, "um I think this works");
    }

    #[test]
    fn test_filter_unknown_evidence_removes_universal_keeps_gated() {
        let filtered = remove_filler_words(
            "uhh bueno hmm creo que um ha llegado",
            &OutputLanguageEvidence::Unknown,
            &None,
            true,
        );
        assert_eq!(
            normalize_transcription_output(&filtered),
            "bueno creo que um ha llegado"
        );

        let cyrillic = remove_filler_words(
            "хм я думаю ммм это работает",
            &OutputLanguageEvidence::Unknown,
            &None,
            true,
        );
        assert_eq!(
            normalize_transcription_output(&cyrillic),
            "я думаю это работает"
        );
    }

    #[test]
    fn test_filter_german_gated_fillers_require_evidence() {
        let text = "äh ich glaube ähm das passt";

        let unknown = remove_filler_words(text, &OutputLanguageEvidence::Unknown, &None, true);
        assert_eq!(normalize_transcription_output(&unknown), text);

        let result = filter_transcription_output(text, "de", &None);
        assert_eq!(result, "ich glaube das passt");
    }

    #[test]
    fn test_filter_preserves_millimetre_unit() {
        // "mm" was removed from the filler lists because it eats units.
        let text = "the screw is 5 mm long";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "the screw is 5 mm long");
    }

    #[test]
    fn test_filter_detected_evidence_unlocks_gated_fillers() {
        let model = remove_filler_words(
            "um I think this works",
            &OutputLanguageEvidence::ModelDetected("en".to_string()),
            &None,
            true,
        );
        assert_eq!(normalize_transcription_output(&model), "I think this works");

        let text = remove_filler_words(
            "euh je pense que ça marche",
            &OutputLanguageEvidence::TextDetected("fr".to_string()),
            &None,
            true,
        );
        assert_eq!(
            normalize_transcription_output(&text),
            "je pense que ça marche"
        );
    }

    #[test]
    fn test_filter_master_toggle_disables_custom_and_builtin_removal() {
        let text = "um customword I think";
        let language = OutputLanguageEvidence::UserSelected("en".to_string());
        let custom = Some(vec!["customword".to_string()]);

        let result = remove_filler_words(text, &language, &custom, false);

        assert_eq!(result, text);
    }

    #[test]
    fn test_filter_custom_words_apply_without_language_evidence() {
        let custom = Some(vec!["customword".to_string()]);
        let text = "customword should be removed but um should remain";

        let filtered = remove_filler_words(text, &OutputLanguageEvidence::Unknown, &custom, true);
        let result = normalize_transcription_output(&filtered);

        assert_eq!(result, "should be removed but um should remain");
    }

    #[test]
    fn test_apply_custom_words_ngram_two_words() {
        let text = "il cui nome è Charge B, che permette";
        let custom_words = vec!["ChargeBee".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert!(result.contains("ChargeBee,"), "unexpected result: {result}");
        assert!(!result.contains("Charge B"));
    }

    #[test]
    fn test_apply_custom_words_ngram_three_words() {
        let text = "use Chat G P T for this";
        let custom_words = vec!["ChatGPT".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert!(result.contains("ChatGPT"));
    }

    #[test]
    fn test_apply_custom_words_prefers_longer_ngram() {
        let text = "Open AI GPT model";
        let custom_words = vec!["OpenAI".to_string(), "GPT".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert_eq!(result, "OpenAI GPT model");
    }

    #[test]
    fn test_apply_custom_words_ngram_preserves_case() {
        let text = "CHARGE B is great";
        let custom_words = vec!["ChargeBee".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert!(result.contains("CHARGEBEE"));
    }

    #[test]
    fn test_apply_custom_words_ngram_with_spaces_in_custom() {
        // Custom word with space should also match against split words
        let text = "using Mac Book Pro";
        let custom_words = vec!["MacBook Pro".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert_eq!(result, "using MacBook Pro");
    }

    #[test]
    fn test_apply_custom_words_trailing_number_not_doubled() {
        // Verify that trailing non-alpha chars (like numbers) aren't double-counted
        // between build_ngram stripping them and extract_punctuation capturing them
        let text = "use GPT4 for this";
        let custom_words = vec!["GPT-4".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        // Should NOT produce "GPT-44" (double-counting the trailing 4)
        assert!(
            !result.contains("GPT-44"),
            "got double-counted result: {}",
            result
        );
    }

    #[test]
    fn test_apply_custom_words_matches_ampersand_word() {
        let text = "send it to RD for review";
        let custom_words = vec!["R&D".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.18);
        assert_eq!(result, "send it to R&D for review");
    }

    #[test]
    fn test_apply_custom_words_matches_spoken_ampersand_word() {
        let text = "send it to R and D for review";
        let custom_words = vec!["R&D".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.18);
        assert_eq!(result, "send it to R&D for review");
    }

    #[test]
    fn test_apply_custom_words_preserves_ampersand_word() {
        let text = "send it to R&D for review";
        let custom_words = vec!["R&D".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.18);
        assert_eq!(result, "send it to R&D for review");
    }

    #[test]
    fn test_apply_custom_words_handles_unicode_punctuation() {
        let text = "「Handee。」";
        let custom_words = vec!["Handy".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert_eq!(result, "「Handy。」");
    }

    #[test]
    fn test_apply_custom_words_skips_cjk_fuzzy_matching() {
        let text = "你好。";
        let custom_words = vec!["你号".to_string()];
        let result = apply_custom_words(text, &custom_words, 1.0);
        assert_eq!(result, text);
    }

    fn spoken_french(text: &str) -> String {
        apply_spoken_punctuation(
            text,
            &OutputLanguageEvidence::ModelDetected("fr".to_string()),
            true,
        )
    }

    #[test]
    fn test_spoken_punctuation_comma_and_new_line() {
        assert_eq!(
            spoken_french("bonjour virgule à la ligne je voulais te dire merci"),
            "bonjour,\nJe voulais te dire merci"
        );
    }

    #[test]
    fn test_spoken_punctuation_drops_model_punctuation_around_commands() {
        assert_eq!(
            spoken_french("Bonjour, virgule, à la ligne. Je voulais te dire merci."),
            "Bonjour,\nJe voulais te dire merci."
        );
    }

    #[test]
    fn test_spoken_punctuation_keeps_sentence_end_before_new_line() {
        assert_eq!(
            spoken_french("Merci. À la ligne. Cordialement, Romain."),
            "Merci.\nCordialement, Romain."
        );
    }

    #[test]
    fn test_spoken_punctuation_period_and_question_mark() {
        assert_eq!(
            spoken_french("je suis content point comment ça va point d'interrogation"),
            "je suis content. Comment ça va ?"
        );
    }

    #[test]
    fn test_spoken_punctuation_new_paragraph_and_repeated_lines() {
        assert_eq!(
            spoken_french("Salut nouveau paragraphe merci à la ligne à la ligne ça va"),
            "Salut\n\nMerci\n\nÇa va"
        );
    }

    #[test]
    fn test_spoken_punctuation_colon_semicolon_exclamation() {
        assert_eq!(
            spoken_french("Il y a deux options, deux points, la première point-virgule la seconde point d'exclamation"),
            "Il y a deux options : la première ; la seconde !"
        );
    }

    #[test]
    fn test_spoken_punctuation_parentheses_and_quotes() {
        assert_eq!(
            spoken_french("J'ai vu Pierre, ouvrez la parenthèse, mon frère, fermez la parenthèse, hier."),
            "J'ai vu Pierre (mon frère) hier."
        );
        assert_eq!(
            spoken_french("Il m'a dit ouvrez les guillemets bonjour fermez les guillemets"),
            "Il m'a dit « bonjour »"
        );
    }

    #[test]
    fn test_spoken_punctuation_keeps_trailing_new_line() {
        assert_eq!(spoken_french("Cordialement à la ligne"), "Cordialement\n");
    }

    #[test]
    fn test_spoken_punctuation_preserves_ordinary_uses_of_point() {
        for text in [
            "C'est le point important.",
            "Quel est ton point de vue ?",
            "Il faut mettre un point final à cette histoire.",
            "J'ai marqué deux points de plus.",
            "Une nouvelle ligne de bus ouvre.",
            "Écris-moi sur gmail point com",
            "Il habite à Point-à-Pitre",
            "C'est un point d'honneur.",
        ] {
            assert_eq!(spoken_french(text), text);
        }
    }

    #[test]
    fn test_spoken_punctuation_leaves_untouched_text_verbatim() {
        assert_eq!(
            spoken_french("Ça coûte 3,5 euros virgule voir gmail.com"),
            "Ça coûte 3,5 euros, voir gmail.com"
        );
    }

    #[test]
    fn test_spoken_punctuation_ambiguous_commands_need_french_or_unknown() {
        let english = OutputLanguageEvidence::ModelDetected("en".to_string());
        assert_eq!(
            apply_spoken_punctuation("the answer point done", &english, true),
            "the answer point done"
        );
        assert_eq!(
            apply_spoken_punctuation("the point is simple", &OutputLanguageEvidence::Unknown, true),
            "the point is simple"
        );
        // Unambiguous French commands still apply.
        assert_eq!(
            apply_spoken_punctuation("hello virgule world", &english, true),
            "hello, world"
        );
    }

    #[test]
    fn test_spoken_punctuation_disabled() {
        let text = "bonjour virgule à la ligne";
        assert_eq!(
            apply_spoken_punctuation(text, &OutputLanguageEvidence::Unknown, false),
            text
        );
    }

    #[test]
    fn test_spoken_punctuation_survives_normalization_order() {
        // Normalization collapses whitespace, so spoken punctuation must run
        // after it for line breaks to survive.
        let normalized = normalize_transcription_output("  Bonjour   à la ligne   Pierre  ");
        assert_eq!(spoken_french(&normalized), "Bonjour\nPierre");
    }
}
