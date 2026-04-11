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

/// Get all builtin DLP patterns
pub fn get_builtin_patterns() -> &'static [BuiltinPattern] {
    &[
        BuiltinPattern {
            name: "API Keys",
            pattern_type: "regex",
            patterns: &[
                // OpenAI
                r"sk-[a-zA-Z0-9]{20,}",
                r"sk-proj-[a-zA-Z0-9\-_]{20,}",
                // Anthropic
                r"sk-ant-[a-zA-Z0-9\-_]{20,}",
                // AWS Access Key ID
                r"AKIA[0-9A-Z]{16}",
                // GitHub tokens
                r"ghp_[a-zA-Z0-9]{36}",
                r"gho_[a-zA-Z0-9]{36}",
                r"ghu_[a-zA-Z0-9]{36}",
                r"ghs_[a-zA-Z0-9]{36}",
                r"ghr_[a-zA-Z0-9]{36}",
                r"github_pat_[a-zA-Z0-9_]{22,}",
                // Slack tokens
                r"xox[baprs]-[a-zA-Z0-9\-]{10,}",
                r"xapp-[0-9]+-[A-Za-z0-9\-]+",
                // Stripe keys
                r"sk_live_[a-zA-Z0-9]{24,}",
                r"sk_test_[a-zA-Z0-9]{24,}",
                r"pk_live_[a-zA-Z0-9]{24,}",
                r"pk_test_[a-zA-Z0-9]{24,}",
                r"rk_live_[a-zA-Z0-9]{24,}",
                r"rk_test_[a-zA-Z0-9]{24,}",
                // Google
                r"AIza[0-9A-Za-z\-_]{35}",
                r"ya29\.[0-9A-Za-z\-_]+",
                // Private keys
                r"-----BEGIN\s+(RSA\s+)?PRIVATE\s+KEY-----",
                r"-----BEGIN\s+OPENSSH\s+PRIVATE\s+KEY-----",
                r"-----BEGIN\s+EC\s+PRIVATE\s+KEY-----",
                r"-----BEGIN\s+DSA\s+PRIVATE\s+KEY-----",
                r"-----BEGIN\s+PGP\s+PRIVATE\s+KEY\s+BLOCK-----",
                // SendGrid
                r"SG\.[a-zA-Z0-9_\-]{22,}\.[a-zA-Z0-9_\-]{22,}",
                // Twilio
                r"AC[a-f0-9]{32}",
                r"SK[a-f0-9]{32}",
                // Discord webhooks
                r"https://discord(?:app)?\.com/api/webhooks/\d+/[A-Za-z0-9_\-]+",
                // Slack webhooks
                r"https://hooks\.slack\.com/services/T[A-Z0-9]+/B[A-Z0-9]+/[a-zA-Z0-9]+",
                // JWT tokens
                r"eyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
            ],
            negative_pattern_type: Some("keyword"),
            negative_patterns: Some(COMMON_NEGATIVE_KEYWORDS),
            min_occurrences: 1,
            min_unique_chars: 10,
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
        },
        // ── Database Credentials ─────────────────────────────────────
        BuiltinPattern {
            name: "Database Credentials",
            pattern_type: "regex",
            patterns: &[
                // Connection strings
                r"postgres(?:ql)?://\S{10,}",
                r"mysql://\S{10,}",
                r"mongodb(?:\+srv)?://\S{10,}",
                r"redis://:\S{6,}@\S+",
                r"(?i)Server=[^;]+;.*(?:Password|Pwd)=[^;]{4,}",
                r"jdbc:[a-z]+://\S{10,}",
                // Password assignments
                r"(?i)(?:db|database|postgres|mysql|mongo|redis|mssql|sql)_?password\s*[=:]\s*\S{8,}",
            ],
            negative_pattern_type: Some("regex"),
            negative_patterns: Some(&[
                r"(?i)example\.com|localhost|127\.0\.0\.1|placeholder|dummy|fake|your.|sample|template|password123|changeme|xxxx|test_?db|mock|todo|\$\{|%s|\{\{",
            ]),
            min_occurrences: 1,
            min_unique_chars: 8,
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
        },
    ]
}
