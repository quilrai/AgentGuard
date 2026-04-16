// Hardcoded builtin DLP patterns
// This replaces the JSON file to avoid bundling external files

/// Builtin pattern definition
pub struct BuiltinPattern {
    pub name: &'static str,
    pub pattern_type: &'static str,
    pub patterns: &'static [&'static str],
    pub negative_pattern_type: Option<&'static str>,
    pub negative_patterns: Option<&'static [&'static str]>,
    pub min_occurrences: i32,
    pub min_unique_chars: i32,
    /// Optional post-match validator function. Called with the raw matched string
    /// (whitespace/separators stripped as needed). Return true to keep the match.
    /// Used for checksum validation (Luhn, Verhoeff, etc.).
    pub validator: Option<fn(&str) -> bool>,
    /// Key to look up the validator at runtime when loading from DB.
    /// Must match a key in `get_validator_by_name()`.
    pub validator_name: Option<&'static str>,
}

// Common negative patterns to exclude dummy/placeholder/example values
// These match in a 30-char context window around the detected value
const COMMON_NEGATIVE_KEYWORDS: &[&str] = &[
    "example",
    "sample",
    "placeholder",
    "dummy",
    "fake",
    "test",
    "xxx",
    "your_",
    "your-",
    "insert",
    "replace",
    "change_me",
    "CHANGE_ME",
    "todo",
    "fixme",
    "<your",
    "{your",
    "mock",
    "template",
];

// ═══════════════════════════════════════════════════════════════════════
// Checksum validators
// ═══════════════════════════════════════════════════════════════════════

/// Resolve a validator function by name. Used when loading patterns from the
/// database — the DB stores the validator_name string, and we look it up here.
pub fn get_validator_by_name(name: &str) -> Option<fn(&str) -> bool> {
    match name {
        "iban" => Some(validate_iban),
        "luhn" => Some(validate_luhn),
        "uk_nino" => Some(validate_uk_nino),
        "verhoeff" => Some(validate_verhoeff),
        _ => None,
    }
}

/// Strip spaces and dashes from a matched string to get raw digits.
fn strip_separators(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

// ── Luhn algorithm (ISO/IEC 7812) ──────────────────────────────────────
// Used for credit/debit card number validation.

pub fn validate_luhn(matched: &str) -> bool {
    let digits: Vec<u32> = strip_separators(matched)
        .chars()
        .filter_map(|c| c.to_digit(10))
        .collect();

    if digits.len() < 12 {
        return false;
    }

    let mut sum: u32 = 0;
    let len = digits.len();
    for (i, &d) in digits.iter().rev().enumerate() {
        if i % 2 == 1 {
            let doubled = d * 2;
            sum += if doubled > 9 { doubled - 9 } else { doubled };
        } else {
            sum += d;
        }
    }

    // Also reject all-same-digit sequences (e.g. 4444444444444444)
    let all_same = digits.windows(2).all(|w| w[0] == w[1]);
    if all_same {
        return false;
    }

    sum % 10 == 0 && len >= 13
}

// ── Verhoeff algorithm ─────────────────────────────────────────────────
// Used for Aadhaar number validation (12-digit Indian unique ID).
// The Verhoeff checksum catches all single-digit errors AND all adjacent
// transposition errors — stronger than Luhn for numeric-only IDs.

// Multiplication table d
const VERHOEFF_D: [[u8; 10]; 10] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
    [1, 2, 3, 4, 0, 6, 7, 8, 9, 5],
    [2, 3, 4, 0, 1, 7, 8, 9, 5, 6],
    [3, 4, 0, 1, 2, 8, 9, 5, 6, 7],
    [4, 0, 1, 2, 3, 9, 5, 6, 7, 8],
    [5, 9, 8, 7, 6, 0, 4, 3, 2, 1],
    [6, 5, 9, 8, 7, 1, 0, 4, 3, 2],
    [7, 6, 5, 9, 8, 2, 1, 0, 4, 3],
    [8, 7, 6, 5, 9, 3, 2, 1, 0, 4],
    [9, 8, 7, 6, 5, 4, 3, 2, 1, 0],
];

// Permutation table p
const VERHOEFF_P: [[u8; 10]; 8] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
    [1, 5, 7, 6, 2, 8, 3, 0, 9, 4],
    [5, 8, 0, 3, 7, 9, 6, 1, 4, 2],
    [8, 9, 1, 6, 0, 4, 3, 5, 2, 7],
    [9, 4, 5, 3, 1, 2, 6, 8, 7, 0],
    [4, 2, 8, 6, 5, 7, 3, 9, 0, 1],
    [2, 7, 9, 3, 8, 0, 6, 4, 1, 5],
    [7, 0, 4, 6, 9, 1, 3, 2, 5, 8],
];

pub fn validate_verhoeff(matched: &str) -> bool {
    let digits: Vec<u8> = strip_separators(matched)
        .chars()
        .filter_map(|c| c.to_digit(10).map(|d| d as u8))
        .collect();

    if digits.len() != 12 {
        return false;
    }

    // Reject obvious fakes: all same digit, sequential
    let all_same = digits.windows(2).all(|w| w[0] == w[1]);
    if all_same {
        return false;
    }
    let ascending: Vec<u8> = (0..12)
        .map(|i| ((digits[0] as u16 + i) % 10) as u8)
        .collect();
    if digits == ascending {
        return false;
    }

    let mut c: u8 = 0;
    for (i, &digit) in digits.iter().rev().enumerate() {
        let p_idx = i % 8;
        let p_val = VERHOEFF_P[p_idx][digit as usize];
        c = VERHOEFF_D[c as usize][p_val as usize];
    }

    c == 0
}

// ── IBAN checksum (ISO 13616 / MOD 97-10) ────────────────────────────
// Used to reject account-number-shaped false positives.

pub fn validate_iban(matched: &str) -> bool {
    let compact = strip_separators(matched).to_ascii_uppercase();

    if compact.len() < 15 || compact.len() > 34 {
        return false;
    }

    if !compact.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }

    let mut rearranged = String::with_capacity(compact.len());
    rearranged.push_str(&compact[4..]);
    rearranged.push_str(&compact[..4]);

    let mut remainder: u32 = 0;
    for ch in rearranged.chars() {
        match ch {
            '0'..='9' => {
                remainder = (remainder * 10 + ch.to_digit(10).unwrap_or(0)) % 97;
            }
            'A'..='Z' => {
                let val = (ch as u32) - ('A' as u32) + 10;
                remainder = (remainder * 100 + val) % 97;
            }
            _ => return false,
        }
    }

    remainder == 1
}

// ── UK National Insurance Number validity rules ──────────────────────
// The regex handles the broad shape; this rejects known invalid prefixes
// and obvious fake serials such as all-zero digits.

pub fn validate_uk_nino(matched: &str) -> bool {
    let compact = strip_separators(matched).to_ascii_uppercase();

    if compact.len() != 9 {
        return false;
    }

    let bytes = compact.as_bytes();
    let first = bytes[0] as char;
    let second = bytes[1] as char;
    let suffix = bytes[8] as char;

    if matches!(first, 'D' | 'F' | 'I' | 'Q' | 'U' | 'V')
        || matches!(second, 'D' | 'F' | 'I' | 'Q' | 'U' | 'V')
    {
        return false;
    }

    let prefix = &compact[..2];
    if matches!(prefix, "BG" | "GB" | "NK" | "KN" | "TN" | "NT" | "ZZ") {
        return false;
    }

    if !matches!(suffix, 'A' | 'B' | 'C' | 'D') {
        return false;
    }

    let digits = &compact[2..8];
    digits.chars().all(|c| c.is_ascii_digit()) && digits != "000000"
}

// ═══════════════════════════════════════════════════════════════════════
// Pattern definitions
// ═══════════════════════════════════════════════════════════════════════

/// Get all builtin DLP patterns
pub fn get_builtin_patterns() -> &'static [BuiltinPattern] {
    &[
        // ── API Keys ────────────────────────────────────────────────────
        BuiltinPattern {
            name: "API Keys",
            pattern_type: "regex",
            patterns: &[
                // OpenAI
                r"\bsk-[a-zA-Z0-9]{20,}\b",
                r"\bsk-proj-[a-zA-Z0-9\-_]{20,}\b",
                // Anthropic
                r"\bsk-ant-[a-zA-Z0-9\-_]{20,}\b",
                // AWS Access Key ID
                r"\bAKIA[0-9A-Z]{16}\b",
                // GitHub tokens
                r"\bghp_[a-zA-Z0-9]{36}\b",
                r"\bgho_[a-zA-Z0-9]{36}\b",
                r"\bghu_[a-zA-Z0-9]{36}\b",
                r"\bghs_[a-zA-Z0-9]{36}\b",
                r"\bghr_[a-zA-Z0-9]{36}\b",
                r"\bgithub_pat_[a-zA-Z0-9_]{22,}\b",
                // Slack tokens
                r"\bxox[baprs]-[a-zA-Z0-9\-]{10,}\b",
                r"\bxapp-[0-9]+-[A-Za-z0-9\-]+\b",
                // Stripe keys
                r"\bsk_live_[a-zA-Z0-9]{24,}\b",
                r"\bsk_test_[a-zA-Z0-9]{24,}\b",
                r"\bpk_live_[a-zA-Z0-9]{24,}\b",
                r"\bpk_test_[a-zA-Z0-9]{24,}\b",
                r"\brk_live_[a-zA-Z0-9]{24,}\b",
                r"\brk_test_[a-zA-Z0-9]{24,}\b",
                // Google
                r"\bAIza[0-9A-Za-z\-_]{35}\b",
                r"\bya29\.[0-9A-Za-z\-_]+\b",
                // Private keys
                r"-----BEGIN\s+(RSA\s+)?PRIVATE\s+KEY-----",
                r"-----BEGIN\s+OPENSSH\s+PRIVATE\s+KEY-----",
                r"-----BEGIN\s+EC\s+PRIVATE\s+KEY-----",
                r"-----BEGIN\s+DSA\s+PRIVATE\s+KEY-----",
                r"-----BEGIN\s+PGP\s+PRIVATE\s+KEY\s+BLOCK-----",
                // SendGrid
                r"\bSG\.[a-zA-Z0-9_\-]{22,}\.[a-zA-Z0-9_\-]{22,}\b",
                // Twilio
                r"\bAC[a-f0-9]{32}\b",
                r"\bSK[a-f0-9]{32}\b",
                // Discord webhooks
                r"https://discord(?:app)?\.com/api/webhooks/\d+/[A-Za-z0-9_\-]+",
                // Slack webhooks
                r"https://hooks\.slack\.com/services/T[A-Z0-9]+/B[A-Z0-9]+/[a-zA-Z0-9]+",
                // JWT tokens
                r"\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b",
            ],
            negative_pattern_type: Some("keyword"),
            negative_patterns: Some(COMMON_NEGATIVE_KEYWORDS),
            min_occurrences: 1,
            min_unique_chars: 10,
            validator: None,
            validator_name: None,
        },
        // ── AWS Secret Keys (need assignment context) ────────────────
        BuiltinPattern {
            name: "AWS Credentials",
            pattern_type: "regex",
            patterns: &[
                r"(?i)(?:aws_secret_access_key|aws_secret|secret_access_key)\s*[=:]\s*[A-Za-z0-9/+=]{40}",
            ],
            negative_pattern_type: Some("regex"),
            negative_patterns: Some(&[
                r"(?i)example|sample|placeholder|dummy|fake|test|xxx|your.|insert|replace|change.me|todo|fixme|mock|template|wJalrXUtnFEMI",
            ]),
            min_occurrences: 1,
            min_unique_chars: 12,
            validator: None,
            validator_name: None,
        },
        // ── Database Credentials ─────────────────────────────────────
        BuiltinPattern {
            name: "Database Credentials",
            pattern_type: "regex",
            patterns: &[
                // Connection strings with embedded credentials
                r#"\bpostgres(?:ql)?://[^:/\s@]+:[^@\s"'`\\\[\]\{\}<>,]{6,}@[^/\s"'`\\\[\]\{\}<>]+(?:/[^\s"'`\\]*)?"#,
                r#"\bmysql://[^:/\s@]+:[^@\s"'`\\\[\]\{\}<>,]{6,}@[^/\s"'`\\\[\]\{\}<>]+(?:/[^\s"'`\\]*)?"#,
                r#"\bmongodb(?:\+srv)?://[^:/\s@]+:[^@\s"'`\\\[\]\{\}<>,]{6,}@[^/\s"'`\\\[\]\{\}<>]+(?:/[^\s"'`\\]*)?"#,
                r#"(?i)\bServer=[A-Za-z0-9._:,\\-]+;(?:[^;\n\r]*;)*(?:Password|Pwd)=[^;\s"'`\\\[\]\{\}<>,]{4,}"#,
                r#"(?i)\bjdbc:[a-z0-9]+://[^\s"'`\\]+(?:\?|;)[^\n\r]*(?:password|pwd)=[^&;\s"'`\\\[\]\{\}<>,]{4,}"#,
                // Password assignments
                r#"(?i)\b(?:db|database|postgres|mysql|mongo|redis|mssql|sql)_?password\s*[=:]\s*["']?[^,\s"'`\\\[\]\{\}<>]{8,}["']?"#,
            ],
            negative_pattern_type: Some("regex"),
            negative_patterns: Some(&[
                r"(?i)example\.com|localhost|127\.0\.0\.1|placeholder|dummy|fake|your.|sample|template|password123|changeme|xxxx|test_?db|mock|todo|\$\{|%s|\{\{",
            ]),
            min_occurrences: 1,
            min_unique_chars: 8,
            validator: None,
            validator_name: None,
        },
        // ── Redis Credentials ────────────────────────────────────────
        BuiltinPattern {
            name: "Redis Credentials",
            pattern_type: "regex",
            patterns: &[
                r#"\brediss?://(?:[^:/\s@]+:|:)[^@\s"'`\\\[\]\{\}<>,]{6,}@[^/\s"'`\\\[\]\{\}<>]+(?:/[^\s"'`\\]*)?"#,
            ],
            negative_pattern_type: Some("regex"),
            negative_patterns: Some(&[
                r"(?i)example\.com|localhost|127\.0\.0\.1|placeholder|dummy|fake|your.|sample|template|password123|changeme|xxxx|test_?db|mock|todo|\$\{|%s|\{\{",
            ]),
            min_occurrences: 1,
            min_unique_chars: 8,
            validator: None,
            validator_name: None,
        },
        // ── Generic Secrets (env var / config assignments) ───────────
        BuiltinPattern {
            name: "Generic Secret Assignments",
            pattern_type: "regex",
            patterns: &[
                r"(?i)(?:secret_key|api_secret|auth_token|access_token|api_token|private_key|encryption_key|signing_key)\s*[=:]\s*[A-Za-z0-9/+=\-_]{16,}",
            ],
            negative_pattern_type: Some("regex"),
            negative_patterns: Some(&[
                r"(?i)example|placeholder|dummy|fake|your.|sample|template|xxxx|mock|todo|fixme|os\.environ|os\.getenv|process\.env|\$\{|%s|\{\{|ENV\[|config\.",
            ]),
            min_occurrences: 1,
            min_unique_chars: 10,
            validator: None,
            validator_name: None,
        },
        // ═════════════════════════════════════════════════════════════
        // PII — India
        // ═════════════════════════════════════════════════════════════

        // ── Aadhaar Number (India) ──────────────────────────────────
        // 12 digits, first digit 2-9, with Verhoeff checksum validation.
        // Matches: 2345 6789 0123 | 2345-6789-0123 | 234567890123
        BuiltinPattern {
            name: "Aadhaar Number (India)",
            pattern_type: "regex",
            patterns: &[r"\b[2-9][0-9]{3}[\s\-]?[0-9]{4}[\s\-]?[0-9]{4}\b"],
            negative_pattern_type: Some("regex"),
            negative_patterns: Some(&[
                r"(?i)example|sample|placeholder|dummy|fake|test|xxxx|mock|template|0000.?0000.?0000|1111.?1111.?1111|1234.?5678.?9012",
            ]),
            min_occurrences: 1,
            min_unique_chars: 4,
            validator: Some(validate_verhoeff),
            validator_name: Some("verhoeff"),
        },
        // ── PAN Card (India) ────────────────────────────────────────
        // Format: [A-Z]{3}[PCHABGJLFT][A-Z][0-9]{4}[A-Z]
        // 4th char encodes entity type: P=Person, C=Company, H=HUF,
        // A=AOP, B=BOI, G=Govt, J=AJP, L=Local Auth, F=Firm, T=Trust
        BuiltinPattern {
            name: "PAN Card (India)",
            pattern_type: "regex",
            patterns: &[r"\b[A-Z]{3}[PCHABGJLFT][A-Z][0-9]{4}[A-Z]\b"],
            negative_pattern_type: Some("regex"),
            negative_patterns: Some(&[
                r"(?i)example|sample|placeholder|dummy|fake|test|mock|template|ABCDE1234F|AAAAA\d{4}A",
            ]),
            min_occurrences: 1,
            min_unique_chars: 4,
            validator: None,
            validator_name: None,
        },
        // ── Indian Passport Number ──────────────────────────────────
        // Format: [A-Z][0-9]{7} — letter prefix + 7 digits.
        // Post-2000 series starts with J-Z. Requires "passport" context
        // nearby to avoid false positives on random alphanumeric strings.
        BuiltinPattern {
            name: "Indian Passport Number",
            pattern_type: "regex",
            patterns: &[
                r"(?i)(?:passport|travel\s*doc(?:ument)?|pp\s*no|passport\s*(?:no|number|#))\s*[:\-]?\s*[J-Zj-z][0-9]{7}\b",
            ],
            negative_pattern_type: Some("regex"),
            negative_patterns: Some(&[
                r"(?i)example|sample|placeholder|dummy|fake|test|mock|template",
            ]),
            min_occurrences: 1,
            min_unique_chars: 4,
            validator: None,
            validator_name: None,
        },
        // ── Voter ID / EPIC (India) ─────────────────────────────────
        // Format: 3 uppercase letters (state code) + 7 digits
        BuiltinPattern {
            name: "Voter ID / EPIC (India)",
            pattern_type: "regex",
            patterns: &[
                r"(?i)(?:voter\s*id|epic|election\s*(?:card|id))\s*[:\-#]?\s*[A-Za-z]{3}[0-9]{7}\b",
            ],
            negative_pattern_type: Some("regex"),
            negative_patterns: Some(&[
                r"(?i)example|sample|placeholder|dummy|fake|test|mock|template",
            ]),
            min_occurrences: 1,
            min_unique_chars: 4,
            validator: None,
            validator_name: None,
        },
        // ── Driving License (India) ─────────────────────────────────
        // Format: SS-RR-YYYY-NNNNNNN  (State 2 + RTO 2 + Year 4 + Serial 7)
        // or without separators: SSRRYYYY0000000
        BuiltinPattern {
            name: "Driving License (India)",
            pattern_type: "regex",
            patterns: &[r"\b[A-Z]{2}[\-\s]?[0-9]{2}[\-\s]?(?:19|20)[0-9]{2}[\-\s]?[0-9]{7}\b"],
            negative_pattern_type: Some("regex"),
            negative_patterns: Some(&[
                r"(?i)example|sample|placeholder|dummy|fake|test|mock|template",
            ]),
            min_occurrences: 1,
            min_unique_chars: 4,
            validator: None,
            validator_name: None,
        },
        // ═════════════════════════════════════════════════════════════
        // PII — USA
        // ═════════════════════════════════════════════════════════════

        // ── Social Security Number (USA) ────────────────────────────
        // Format: XXX-XX-XXXX with dashes or spaces.
        // Area (first 3): not 000, 666, or 900-999.
        // Group (middle 2): not 00. Serial (last 4): not 0000.
        BuiltinPattern {
            name: "Social Security Number (USA)",
            pattern_type: "regex",
            patterns: &[
                // With separators (dashes or spaces) — high confidence
                // Area: 001-665 | 667-899 (excludes 000, 666, 900-999)
                // Group: 01-99 (excludes 00). Serial: 0001-9999 (excludes 0000)
                r"\b(?:00[1-9]|0[1-9][0-9]|[1-5][0-9]{2}|6[0-57-9][0-9]|66[0-57-9]|[7-8][0-9]{2})[\-\s](?:0[1-9]|[1-9][0-9])[\-\s](?:[0-9]{3}[1-9]|[0-9]{2}[1-9][0-9]|[0-9][1-9][0-9]{2}|[1-9][0-9]{3})\b",
            ],
            negative_pattern_type: Some("regex"),
            negative_patterns: Some(&[
                r"(?i)example|sample|placeholder|dummy|fake|test|mock|template|078[\-\s]05[\-\s]1120|123[\-\s]45[\-\s]6789|219[\-\s]09[\-\s]9999",
            ]),
            min_occurrences: 1,
            min_unique_chars: 4,
            validator: None,
            validator_name: None,
        },
        // ── US Passport Number ──────────────────────────────────────
        // Format: 9 digits. Requires "passport" keyword in context to
        // avoid matching arbitrary 9-digit numbers.
        BuiltinPattern {
            name: "US Passport Number",
            pattern_type: "regex",
            patterns: &[
                r"(?i)(?:passport|travel\s*doc(?:ument)?|pp\s*no|passport\s*(?:no|number|#))\s*[:\-]?\s*[0-9]{9}\b",
            ],
            negative_pattern_type: Some("regex"),
            negative_patterns: Some(&[
                r"(?i)example|sample|placeholder|dummy|fake|test|mock|template|000000000|123456789",
            ]),
            min_occurrences: 1,
            min_unique_chars: 3,
            validator: None,
            validator_name: None,
        },
        // ── US Individual Taxpayer Identification Number (ITIN) ─────
        // Format: 9XX-XX-XXXX — starts with 9, 4th+5th digits 50-65,70-88,90-92,94-99
        BuiltinPattern {
            name: "ITIN (USA)",
            pattern_type: "regex",
            patterns: &[
                r"\b9[0-9]{2}[\-\s](?:5[0-9]|6[0-5]|7[0-9]|8[0-8]|9[0-2]|9[4-9])[\-\s][0-9]{4}\b",
            ],
            negative_pattern_type: Some("regex"),
            negative_patterns: Some(&[
                r"(?i)example|sample|placeholder|dummy|fake|test|mock|template",
            ]),
            min_occurrences: 1,
            min_unique_chars: 4,
            validator: None,
            validator_name: None,
        },
        // ═════════════════════════════════════════════════════════════
        // PII — Europe
        // ═════════════════════════════════════════════════════════════

        // ── IBAN (International Bank Account Number) ─────────────────
        // Country-specific lengths. We cover the major EU/EEA countries.
        // Format: 2-letter country + 2 check digits + BBAN (alphanumeric)
        BuiltinPattern {
            name: "IBAN (Europe)",
            pattern_type: "regex",
            patterns: &[
                // Germany: DE + 2 check + 18 digits = 22 chars
                r"\bDE[0-9]{2}[\s]?[0-9]{4}[\s]?[0-9]{4}[\s]?[0-9]{4}[\s]?[0-9]{4}[\s]?[0-9]{2}\b",
                // France: FR + 2 check + 10 digits + 11 alphanumeric + 2 digits = 27
                r"\bFR[0-9]{2}[\s]?[0-9]{4}[\s]?[0-9]{4}[\s]?[0-9]{4}[\s]?[0-9]{4}[\s]?[0-9]{4}[\s]?[0-9A-Z]{3}\b",
                // UK: GB + 2 check + 4 alpha + 14 digits = 22
                r"\bGB[0-9]{2}[\s]?[A-Z]{4}[\s]?[0-9]{4}[\s]?[0-9]{4}[\s]?[0-9]{4}[\s]?[0-9]{2}\b",
                // Spain: ES + 2 check + 20 digits = 24
                r"\bES[0-9]{2}[\s]?[0-9]{4}[\s]?[0-9]{4}[\s]?[0-9]{4}[\s]?[0-9]{4}[\s]?[0-9]{4}\b",
                // Italy: IT + 2 check + 1 alpha + 10 digits + 12 alphanumeric = 27
                r"\bIT[0-9]{2}[\s]?[A-Z][0-9]{3}[\s]?[0-9]{4}[\s]?[0-9]{4}[\s]?[0-9]{4}[\s]?[0-9A-Z]{3}\b",
                // Netherlands: NL + 2 check + 4 alpha + 10 digits = 18
                r"\bNL[0-9]{2}[\s]?[A-Z]{4}[\s]?[0-9]{4}[\s]?[0-9]{4}[\s]?[0-9]{2}\b",
                // Belgium: BE + 2 check + 12 digits = 16
                r"\bBE[0-9]{2}[\s]?[0-9]{4}[\s]?[0-9]{4}[\s]?[0-9]{4}\b",
                // Austria: AT + 2 check + 16 digits = 20
                r"\bAT[0-9]{2}[\s]?[0-9]{4}[\s]?[0-9]{4}[\s]?[0-9]{4}[\s]?[0-9]{4}\b",
                // Portugal: PT + 2 check + 21 digits = 25
                r"\bPT[0-9]{2}[\s]?[0-9]{4}[\s]?[0-9]{4}[\s]?[0-9]{4}[\s]?[0-9]{4}[\s]?[0-9]{5}\b",
                // Ireland: IE + 2 check + 4 alpha + 14 digits = 22
                r"\bIE[0-9]{2}[\s]?[A-Z]{4}[\s]?[0-9]{4}[\s]?[0-9]{4}[\s]?[0-9]{4}[\s]?[0-9]{2}\b",
            ],
            negative_pattern_type: Some("regex"),
            negative_patterns: Some(&[
                r"(?i)example|sample|placeholder|dummy|fake|test|mock|template|GB29\s?NWBK|DE89\s?3704|FR76\s?3000",
            ]),
            min_occurrences: 1,
            min_unique_chars: 4,
            validator: Some(validate_iban),
            validator_name: Some("iban"),
        },
        // ── UK National Insurance Number (NIN) ──────────────────────
        // Format: 2 letters + 6 digits + 1 letter (A-D).
        // Excludes invalid prefixes: BG, GB, NK, KN, TN, NT, ZZ, and
        // temp prefixes starting with D, F, I, Q, U, V.
        BuiltinPattern {
            name: "UK National Insurance Number",
            pattern_type: "regex",
            patterns: &[
                // Valid prefix chars: [A-CEGHJ-PR-TW-Z] (excludes D,F,I,Q,U,V)
                // Invalid combos BG,GB,NK,KN,TN,NT,ZZ are rejected by validator
                r"\b[A-CEGHJ-PR-TW-Z]{2}[\s\-]?[0-9]{2}[\s\-]?[0-9]{2}[\s\-]?[0-9]{2}[\s\-]?[A-D]\b",
            ],
            negative_pattern_type: Some("regex"),
            negative_patterns: Some(&[
                r"(?i)example|sample|placeholder|dummy|fake|test|mock|template|AA\s?00\s?00\s?00\s?A",
            ]),
            min_occurrences: 1,
            min_unique_chars: 4,
            validator: Some(validate_uk_nino),
            validator_name: Some("uk_nino"),
        },
        // ── German Tax ID (Steuerliche Identifikationsnummer) ───────
        // 11 digits, first digit non-zero. Requires context keyword to
        // reduce false positives on random 11-digit numbers.
        BuiltinPattern {
            name: "German Tax ID (Steuer-IdNr)",
            pattern_type: "regex",
            patterns: &[
                r"(?i)(?:steuer[\-\s]?id(?:entifikationsnummer)?|tax[\-\s]?id|idnr|tin)\s*[:\-]?\s*[1-9][0-9]{10}\b",
            ],
            negative_pattern_type: Some("regex"),
            negative_patterns: Some(&[
                r"(?i)example|sample|placeholder|dummy|fake|test|mock|template",
            ]),
            min_occurrences: 1,
            min_unique_chars: 3,
            validator: None,
            validator_name: None,
        },
        // ── French NIR (Numéro de sécurité sociale) ─────────────────
        // 13 digits + 2-digit control key. Encodes: gender (1/2), year
        // of birth (2), month (01-12/20+), department (2/3), commune (3),
        // serial (3), key (2).
        BuiltinPattern {
            name: "French NIR (Sécurité Sociale)",
            pattern_type: "regex",
            patterns: &[
                r"\b[12][\s\-]?[0-9]{2}[\s\-]?(?:0[1-9]|1[0-2]|20)[\s\-]?[0-9]{2}[\s\-]?[0-9]{3}[\s\-]?[0-9]{3}[\s\-]?[0-9]{2}\b",
            ],
            negative_pattern_type: Some("regex"),
            negative_patterns: Some(&[
                r"(?i)example|sample|placeholder|dummy|fake|test|mock|template",
            ]),
            min_occurrences: 1,
            min_unique_chars: 4,
            validator: None,
            validator_name: None,
        },
        // ═════════════════════════════════════════════════════════════
        // PII — Global (multi-region)
        // ═════════════════════════════════════════════════════════════

        // ── Credit / Debit Card Numbers ─────────────────────────────
        // Covers Visa, Mastercard, Amex, Discover, RuPay, Maestro.
        // Matches with spaces or dashes as separators.
        // Luhn checksum validation eliminates false positives.
        BuiltinPattern {
            name: "Credit/Debit Card Number",
            pattern_type: "regex",
            patterns: &[
                // Visa: starts with 4, 13 or 16 digits
                r"\b4[0-9]{3}[\s\-]?[0-9]{4}[\s\-]?[0-9]{4}[\s\-]?[0-9]{1,4}\b",
                // Mastercard: 51-55 or 2221-2720 range, 16 digits
                r"\b5[1-5][0-9]{2}[\s\-]?[0-9]{4}[\s\-]?[0-9]{4}[\s\-]?[0-9]{4}\b",
                r"\b2(?:2[2-9][1-9]|2[3-9][0-9]|[3-6][0-9]{2}|7[01][0-9]|720)[\s\-]?[0-9]{4}[\s\-]?[0-9]{4}[\s\-]?[0-9]{4}\b",
                // Amex: starts with 34 or 37, 15 digits
                r"\b3[47][0-9]{2}[\s\-]?[0-9]{6}[\s\-]?[0-9]{5}\b",
                // Discover: 6011, 644-649, 65, 16 digits
                r"\b(?:6011|64[4-9][0-9]|65[0-9]{2})[\s\-]?[0-9]{4}[\s\-]?[0-9]{4}[\s\-]?[0-9]{4}\b",
                // RuPay (India): 60, 65, 81, 82 prefixes, 16 digits
                r"\b(?:60[0-9]{2}|65[0-9]{2}|81[0-9]{2}|82[0-9]{2})[\s\-]?[0-9]{4}[\s\-]?[0-9]{4}[\s\-]?[0-9]{4}\b",
                // Maestro (Europe): 5018, 5020, 5038, 6304, 6759, 6761-6763
                r"\b(?:5018|5020|5038|6304|6759|676[1-3])[\s\-]?[0-9]{4}[\s\-]?[0-9]{4}[\s\-]?[0-9]{0,7}\b",
            ],
            negative_pattern_type: Some("regex"),
            negative_patterns: Some(&[
                // Known test card numbers
                r"(?i)example|sample|placeholder|dummy|fake|test|mock|template|4111[\s\-]?1111[\s\-]?1111[\s\-]?1111|5500[\s\-]?0000[\s\-]?0000[\s\-]?0004|3782[\s\-]?822463[\s\-]?10005|0000[\s\-]?0000",
            ]),
            min_occurrences: 1,
            min_unique_chars: 4,
            validator: Some(validate_luhn),
            validator_name: Some("luhn"),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern_utils::{
        collect_matches_with_negative_context, compile_pattern_set, filter_by_min_occurrences,
    };

    fn matches_for_builtin(name: &str, text: &str) -> Vec<String> {
        let builtin = get_builtin_patterns()
            .iter()
            .find(|bp| bp.name == name)
            .unwrap_or_else(|| panic!("missing builtin pattern: {}", name));

        let patterns: Vec<String> = builtin.patterns.iter().map(|p| (*p).to_string()).collect();
        let negative_patterns = builtin.negative_patterns.map(|patterns| {
            patterns
                .iter()
                .map(|p| (*p).to_string())
                .collect::<Vec<_>>()
        });

        let compiled = compile_pattern_set(
            &patterns,
            builtin.pattern_type,
            negative_patterns.as_ref(),
            builtin.negative_pattern_type,
        )
        .unwrap_or_else(|e| panic!("failed to compile '{}': {}", name, e));

        let match_result = collect_matches_with_negative_context(
            text,
            &compiled.regexes,
            &compiled.negative_regexes,
            builtin.min_unique_chars,
            builtin.validator,
        );

        filter_by_min_occurrences(match_result, builtin.min_occurrences)
    }

    #[test]
    fn test_luhn_valid() {
        // Known valid card numbers
        assert!(validate_luhn("4532015112830366"));
        assert!(validate_luhn("5425233430109903"));
        assert!(validate_luhn("4532 0151 1283 0366")); // with spaces
        assert!(validate_luhn("4532-0151-1283-0366")); // with dashes
    }

    #[test]
    fn test_luhn_invalid() {
        assert!(!validate_luhn("4532015112830367")); // bad check digit
        assert!(!validate_luhn("1234567890123456")); // not valid luhn
        assert!(!validate_luhn("0000000000000000")); // all zeros
        assert!(!validate_luhn("4444444444444444")); // all same
    }

    #[test]
    fn test_luhn_test_cards_valid() {
        // Stripe/industry test cards pass Luhn (blocked by negative patterns instead)
        assert!(validate_luhn("4111111111111111"));
    }

    #[test]
    fn test_verhoeff_valid() {
        // Known valid Aadhaar-like numbers (pass Verhoeff)
        assert!(validate_verhoeff("276598387210"));
        assert!(validate_verhoeff("498123456788"));
        assert!(validate_verhoeff("2765 9838 7210")); // with spaces
    }

    #[test]
    fn test_verhoeff_invalid() {
        assert!(!validate_verhoeff("123456789012")); // sequential
        assert!(!validate_verhoeff("111111111111")); // all same
        assert!(!validate_verhoeff("585293751385")); // bad check digit
    }

    #[test]
    fn test_verhoeff_rejects_short() {
        assert!(!validate_verhoeff("12345"));
        assert!(!validate_verhoeff("1234567890")); // 10 digits
    }

    #[test]
    fn test_iban_valid() {
        assert!(validate_iban("GB82 WEST 1234 5698 7654 32"));
        assert!(validate_iban("DE75512108001245126199"));
    }

    #[test]
    fn test_iban_invalid() {
        assert!(!validate_iban("GB82 WEST 1234 5698 7654 31"));
        assert!(!validate_iban("DE75512108001245126198"));
    }

    #[test]
    fn test_uk_nino_valid() {
        assert!(validate_uk_nino("AB 12 34 56 C"));
        assert!(validate_uk_nino("JR123456D"));
    }

    #[test]
    fn test_uk_nino_invalid() {
        assert!(!validate_uk_nino("BG 12 34 56 A"));
        assert!(!validate_uk_nino("AB 00 00 00 A"));
        assert!(!validate_uk_nino("DQ123456A"));
    }

    #[test]
    fn test_strip_separators() {
        assert_eq!(strip_separators("4532 0151 1283 0366"), "4532015112830366");
        assert_eq!(strip_separators("4532-0151-1283-0366"), "4532015112830366");
        assert_eq!(strip_separators("2345 6789 0123"), "234567890123");
    }

    #[test]
    fn test_all_patterns_compile() {
        let patterns = get_builtin_patterns();
        for bp in patterns {
            for p in bp.patterns {
                regex::Regex::new(p).unwrap_or_else(|e| {
                    panic!("Pattern '{}' in '{}' failed to compile: {}", p, bp.name, e)
                });
            }
            if let Some(neg) = bp.negative_patterns {
                let neg_type = bp.negative_pattern_type.unwrap_or("regex");
                for np in neg {
                    if neg_type == "regex" {
                        regex::Regex::new(np).unwrap_or_else(|e| {
                            panic!(
                                "Negative pattern '{}' in '{}' failed to compile: {}",
                                np, bp.name, e
                            )
                        });
                    }
                }
            }
        }
    }

    #[test]
    fn test_database_credentials_ignore_regex_literals() {
        let text = r#"These are regex patterns, not real credentials:
redis://:\S{6,}@\S+
mysql://\S{10,}
Server=[^;]+;.*(?:Password|Pwd)=[^;]{4,}"#;

        let matches = matches_for_builtin("Database Credentials", text);
        assert!(matches.is_empty(), "unexpected matches: {:?}", matches);
    }

    #[test]
    fn test_database_credentials_require_embedded_credentials() {
        let text = "Connect using mysql://db.prod.internal:3306/appdb for local testing notes.";
        let matches = matches_for_builtin("Database Credentials", text);
        assert!(matches.is_empty(), "unexpected matches: {:?}", matches);
    }

    #[test]
    fn test_database_credentials_match_real_connection_strings_and_assignments() {
        let text = r#"
primary = "postgres://appuser:Sup3rSecret!@db.prod.internal:5432/app"
secondary = "Server=db.prod.internal,1433;Database=app;User Id=sa;Password=Sup3rSecret!"
db_password = "Sup3rSecret!"
"#;

        let matches = matches_for_builtin("Database Credentials", text);
        assert_eq!(matches.len(), 3, "unexpected matches: {:?}", matches);
        assert!(matches
            .iter()
            .any(|m| m.contains("postgres://appuser:Sup3rSecret!@db.prod.internal:5432/app")));
        assert!(matches.iter().any(|m| m.contains(
            "Server=db.prod.internal,1433;Database=app;User Id=sa;Password=Sup3rSecret!"
        )));
        assert!(matches
            .iter()
            .any(|m| m.contains(r#"db_password = "Sup3rSecret!""#)));
    }

    #[test]
    fn test_redis_credentials_ignore_regex_literals_but_match_real_uris() {
        let literal = r#"redis://:\S{6,}@\S+"#;
        let literal_matches = matches_for_builtin("Redis Credentials", literal);
        assert!(
            literal_matches.is_empty(),
            "unexpected matches: {:?}",
            literal_matches
        );

        let real = "redis://:Sup3rSecret!@cache.prod.internal:6379/0";
        let real_matches = matches_for_builtin("Redis Credentials", real);
        assert_eq!(real_matches, vec![real.to_string()]);
    }

    #[test]
    fn test_api_keys_ignore_matches_embedded_in_encoded_blobs() {
        let text =
            "QmFzZTY0VVJMU2VnbWVudF9QcmVmaXhfc2stQUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVowMTIzNDU2Nzg5";
        let matches = matches_for_builtin("API Keys", text);
        assert!(matches.is_empty(), "unexpected matches: {:?}", matches);
    }

    #[test]
    fn test_api_keys_still_match_standalone_jwt() {
        let jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4iLCJhZG1pbiI6dHJ1ZX0.c2lnbmF0dXJlU2VnbWVudEFiY0RlZjEyMzQ1Njc4OTA";
        let matches = matches_for_builtin("API Keys", jwt);
        assert_eq!(matches, vec![jwt.to_string()]);
    }

    #[test]
    fn test_iban_builtin_rejects_bad_checksum() {
        let text = "payment details: GB82 WEST 1234 5698 7654 31";
        let matches = matches_for_builtin("IBAN (Europe)", text);
        assert!(matches.is_empty(), "unexpected matches: {:?}", matches);
    }

    #[test]
    fn test_iban_builtin_accepts_valid_value() {
        let text = "payment details: GB82 WEST 1234 5698 7654 32";
        let matches = matches_for_builtin("IBAN (Europe)", text);
        assert_eq!(matches, vec!["GB82 WEST 1234 5698 7654 32".to_string()]);
    }

    #[test]
    fn test_uk_nino_builtin_rejects_invalid_prefix() {
        let text = "national insurance number: BG 12 34 56 A";
        let matches = matches_for_builtin("UK National Insurance Number", text);
        assert!(matches.is_empty(), "unexpected matches: {:?}", matches);
    }

    #[test]
    fn test_uk_nino_builtin_accepts_valid_value() {
        let text = "national insurance number: AB 12 34 56 C";
        let matches = matches_for_builtin("UK National Insurance Number", text);
        assert_eq!(matches, vec!["AB 12 34 56 C".to_string()]);
    }
}
