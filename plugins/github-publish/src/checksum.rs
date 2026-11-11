//! SHA-256 computation and `checksums.txt` generation for release assets.

use sha2::{Digest, Sha256};

/// Computes the lowercase hexadecimal SHA-256 digest of `bytes`.
pub fn sha256_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let result = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in result {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Formats a GNU/BSD-compatible `checksums.txt` document.
///
/// Each line has the format:
/// `<sha256-hex>  <filename>`
pub fn generate_checksums_file(entries: &[(&str, &str)]) -> String {
    let mut out = String::new();
    for (filename, hash) in entries {
        out.push_str(hash);
        out.push_str("  ");
        out.push_str(filename);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_correct_sha256() {
        // Known test vector: SHA-256 of empty string
        assert_eq!(
            sha256_digest(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        // Known test vector: "hello world"
        assert_eq!(
            sha256_digest(b"hello world"),
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn formats_checksums_file() {
        let entries = [
            ("cutver-x86_64.tar.gz", "1111222233334444"),
            ("cutver-aarch64.tar.gz", "5555666677778888"),
        ];
        let file = generate_checksums_file(&entries);
        assert_eq!(
            file,
            "1111222233334444  cutver-x86_64.tar.gz\n5555666677778888  cutver-aarch64.tar.gz\n"
        );
    }
}
