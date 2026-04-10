use std::path::Path;

pub fn shorten_path(path: &str) -> String {
    let p = Path::new(path);
    if let Some(name) = p.file_name() {
        return name.to_string_lossy().to_string();
    }
    path.to_string()
}

pub fn format_savings(original: usize, compressed: usize) -> String {
    let saved = original.saturating_sub(compressed);
    if original == 0 {
        return "0 tok saved".to_string();
    }
    let pct = (saved as f64 / original as f64 * 100.0).round() as usize;
    format!("[{saved} tok saved ({pct}%)]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorten_extracts_filename() {
        assert_eq!(shorten_path("/foo/bar/main.rs"), "main.rs");
        assert_eq!(shorten_path("main.rs"), "main.rs");
    }

    #[test]
    fn savings_format() {
        assert_eq!(format_savings(100, 60), "[40 tok saved (40%)]");
        assert_eq!(format_savings(0, 0), "0 tok saved");
        assert_eq!(format_savings(100, 100), "[0 tok saved (0%)]");
    }
}
