use std::path::Path;

const SEPARATOR: &str = " - ";
const INCOMPLETE_PREFIX: &str = "INCOMPLETE";

/// Create a rated filename: "85 - Some Video.mp4"
pub fn rated_filename(original: &str, rating: u32) -> Result<String, RenameError> {
    if !(10..=99).contains(&rating) {
        return Err(RenameError::InvalidRating(rating));
    }
    let base = strip_prefix(original);
    Ok(format!("{}{}{}", rating, SEPARATOR, base))
}

/// Create an incomplete filename: "INCOMPLETE - Some Video.mp4"
pub fn incomplete_filename(original: &str) -> String {
    let base = strip_prefix(original);
    format!("{}{}{}", INCOMPLETE_PREFIX, SEPARATOR, base)
}

/// Strip any existing prefix (rating or INCOMPLETE) from a filename
pub fn strip_prefix(filename: &str) -> &str {
    if let Some(pos) = filename.find(SEPARATOR) {
        let prefix = &filename[..pos];
        if prefix == INCOMPLETE_PREFIX || prefix.parse::<u32>().is_ok() {
            return &filename[pos + SEPARATOR.len()..];
        }
    }
    filename
}

/// Parse a rating from a filename, if present
#[allow(dead_code)]
pub fn parse_rating(filename: &str) -> Option<u32> {
    if let Some(pos) = filename.find(SEPARATOR) {
        let prefix = &filename[..pos];
        if let Ok(rating) = prefix.parse::<u32>()
            && (10..=99).contains(&rating)
        {
            return Some(rating);
        }
    }
    None
}

/// Check if a filename has the INCOMPLETE prefix
#[allow(dead_code)]
pub fn is_incomplete(filename: &str) -> bool {
    filename.starts_with(&format!("{}{}", INCOMPLETE_PREFIX, SEPARATOR))
}

/// Rename a file on disk to apply a rating. Returns the new filename.
pub fn apply_rating(
    dir: &Path,
    current_filename: &str,
    rating: u32,
) -> Result<String, RenameError> {
    let new_filename = rated_filename(current_filename, rating)?;
    let old_path = dir.join(current_filename);
    let new_path = dir.join(&new_filename);

    if !old_path.exists() {
        return Err(RenameError::FileNotFound(current_filename.to_string()));
    }

    std::fs::rename(&old_path, &new_path).map_err(RenameError::Io)?;
    Ok(new_filename)
}

#[derive(Debug)]
pub enum RenameError {
    InvalidRating(u32),
    FileNotFound(String),
    Io(std::io::Error),
}

impl std::fmt::Display for RenameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRating(r) => write!(f, "Rating must be 10-99, got {}", r),
            Self::FileNotFound(name) => write!(f, "File not found: {}", name),
            Self::Io(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl std::error::Error for RenameError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_rated_filename() {
        assert_eq!(rated_filename("Video.mp4", 85).unwrap(), "85 - Video.mp4");
        assert_eq!(rated_filename("Video.mp4", 10).unwrap(), "10 - Video.mp4");
        assert_eq!(rated_filename("Video.mp4", 99).unwrap(), "99 - Video.mp4");
    }

    #[test]
    fn test_rated_filename_strips_existing_rating() {
        assert_eq!(
            rated_filename("50 - Video.mp4", 85).unwrap(),
            "85 - Video.mp4"
        );
    }

    #[test]
    fn test_rated_filename_strips_incomplete() {
        assert_eq!(
            rated_filename("INCOMPLETE - Video.mp4", 75).unwrap(),
            "75 - Video.mp4"
        );
    }

    #[test]
    fn test_rated_filename_invalid() {
        assert!(rated_filename("Video.mp4", 9).is_err());
        assert!(rated_filename("Video.mp4", 100).is_err());
        assert!(rated_filename("Video.mp4", 0).is_err());
    }

    #[test]
    fn test_incomplete_filename() {
        assert_eq!(incomplete_filename("Video.mp4"), "INCOMPLETE - Video.mp4");
    }

    #[test]
    fn test_incomplete_filename_strips_existing() {
        assert_eq!(
            incomplete_filename("85 - Video.mp4"),
            "INCOMPLETE - Video.mp4"
        );
    }

    #[test]
    fn test_strip_prefix() {
        assert_eq!(strip_prefix("85 - Video.mp4"), "Video.mp4");
        assert_eq!(strip_prefix("INCOMPLETE - Video.mp4"), "Video.mp4");
        assert_eq!(strip_prefix("Video.mp4"), "Video.mp4");
        assert_eq!(strip_prefix("My - Video.mp4"), "My - Video.mp4"); // "My" is not a valid prefix
    }

    #[test]
    fn test_parse_rating() {
        assert_eq!(parse_rating("85 - Video.mp4"), Some(85));
        assert_eq!(parse_rating("10 - Video.mp4"), Some(10));
        assert_eq!(parse_rating("Video.mp4"), None);
        assert_eq!(parse_rating("INCOMPLETE - Video.mp4"), None);
        assert_eq!(parse_rating("5 - Video.mp4"), None); // out of range
    }

    #[test]
    fn test_is_incomplete() {
        assert!(is_incomplete("INCOMPLETE - Video.mp4"));
        assert!(!is_incomplete("85 - Video.mp4"));
        assert!(!is_incomplete("Video.mp4"));
    }

    #[test]
    fn test_apply_rating() {
        let dir = tempfile::tempdir().unwrap();
        let filename = "Video.mp4";
        fs::write(dir.path().join(filename), b"test").unwrap();

        let new_name = apply_rating(dir.path(), filename, 85).unwrap();
        assert_eq!(new_name, "85 - Video.mp4");
        assert!(dir.path().join("85 - Video.mp4").exists());
        assert!(!dir.path().join("Video.mp4").exists());
    }

    #[test]
    fn test_apply_rating_re_rate() {
        let dir = tempfile::tempdir().unwrap();
        let filename = "50 - Video.mp4";
        fs::write(dir.path().join(filename), b"test").unwrap();

        let new_name = apply_rating(dir.path(), filename, 90).unwrap();
        assert_eq!(new_name, "90 - Video.mp4");
        assert!(dir.path().join("90 - Video.mp4").exists());
    }

    #[test]
    fn test_apply_rating_from_incomplete() {
        let dir = tempfile::tempdir().unwrap();
        let filename = "INCOMPLETE - Video.mp4";
        fs::write(dir.path().join(filename), b"test").unwrap();

        let new_name = apply_rating(dir.path(), filename, 70).unwrap();
        assert_eq!(new_name, "70 - Video.mp4");
        assert!(dir.path().join("70 - Video.mp4").exists());
    }

    #[test]
    fn test_apply_rating_file_not_found() {
        let dir = tempfile::tempdir().unwrap();
        assert!(apply_rating(dir.path(), "nonexistent.mp4", 85).is_err());
    }
}
