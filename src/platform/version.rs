/// Windows build number of the running OS; 0 when it cannot be read
/// (callers treat 0 as unsupported).
pub fn os_build() -> u32 {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let hk = RegKey::predef(HKEY_LOCAL_MACHINE);
    let cur = match hk.open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion") {
        Ok(k) => k,
        Err(_) => return 0,
    };
    match cur.get_value::<String, _>("CurrentBuildNumber") {
        Ok(s) => s.trim().parse().unwrap_or(0),
        Err(_) => 0,
    }
}

/// The PRD's supported floor: Windows 10 build 19041 (capture exclusion).
pub fn is_supported(build: u32) -> bool {
    build >= 19041
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_boundary() {
        assert!(is_supported(19041)); // Win10 2004 — first supported build
        assert!(!is_supported(19040)); // one below the floor
        assert!(is_supported(26100)); // Win11 24H2
    }
}
