/// Extracts Android's reason code and message from `adb install` output, e.g.
/// `adb: failed to install x.apk: Failure [INSTALL_FAILED_UPDATE_INCOMPATIBLE: Existing package …]`.
/// adb may report a failed incremental attempt before the final one; the last failure wins.
pub(crate) fn parse_failure(output: &str) -> Option<(String, String)> {
    let start = output.rfind("Failure [")? + "Failure [".len();
    let inner = &output[start..];
    let inner = &inner[..inner.find(']').unwrap_or(inner.len())];
    let (reason, detail) = inner.split_once(": ").unwrap_or((inner, ""));
    // Some codes repeat themselves: `INSTALL_FAILED_NO_MATCHING_ABIS: INSTALL_FAILED_NO_MATCHING_ABIS: …`.
    let detail = detail
        .strip_prefix(reason)
        .map_or(detail, |d| d.trim_start_matches(": "));
    Some((reason.trim().to_owned(), detail.trim().to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = format!(
            "{}/../../fixtures/android/adb/{name}.txt",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn signature_mismatch() {
        let (reason, detail) = parse_failure(&fixture("install_signature_mismatch")).unwrap();
        assert_eq!(reason, "INSTALL_FAILED_UPDATE_INCOMPATIBLE");
        assert_eq!(
            detail,
            "Existing package dev.mdh.sample signatures do not match newer version; ignoring!"
        );
    }

    #[test]
    fn missing_abi_with_a_repeated_code() {
        let (reason, detail) = parse_failure(&fixture("install_no_matching_abis")).unwrap();
        assert_eq!(reason, "INSTALL_FAILED_NO_MATCHING_ABIS");
        assert_eq!(detail, "Failed to extract native libraries, res=-113");
    }

    #[test]
    fn version_downgrade() {
        let (reason, detail) = parse_failure(&fixture("install_version_downgrade_api36")).unwrap();
        assert_eq!(reason, "INSTALL_FAILED_VERSION_DOWNGRADE");
        assert_eq!(
            detail,
            "Downgrade detected: Update version code 5 is older than current 6"
        );
    }

    #[test]
    fn other_output_is_not_an_install_failure() {
        assert_eq!(parse_failure("adb: device offline"), None);
    }
}
