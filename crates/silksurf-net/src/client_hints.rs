//! SilkSurf's User-Agent Client Hints profile.
//!
//! One table feeds the `Sec-CH-UA`, `Sec-CH-UA-Mobile`, and
//! `Sec-CH-UA-Platform` request headers (`build_http1_request` and
//! `h2_client::build_h2_request`) and silksurf-js `navigator.userAgentData`,
//! so script and request identity agree. Brand versions come from this
//! crate's package version.

use std::sync::LazyLock;

/// One `NavigatorUABrandVersion` entry (UA-CH "create brands").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BrandVersion {
    pub brand: &'static str,
    pub version: &'static str,
}

/// GREASE brand from UA-CH "create an arbitrary brand": ASCII alpha, one
/// space, `_`, at most twenty bytes, and no leading or trailing space.
const GREASE_BRAND: &str = "Not_A Brand";

/// Significant-version brand list. UA-CH section 8.2 requires more than one
/// brand with one arbitrary value; the order stays fixed per significant
/// version, which the spec's caching note permits.
pub const BRANDS: [BrandVersion; 2] = [
    BrandVersion {
        brand: "SilkSurf",
        version: env!("CARGO_PKG_VERSION_MAJOR"),
    },
    BrandVersion {
        brand: GREASE_BRAND,
        version: "8",
    },
];

/// Full-version brand list; the GREASE entry matches the format of the
/// SilkSurf version and carries a different value.
pub const FULL_VERSION_LIST: [BrandVersion; 2] = [
    BrandVersion {
        brand: "SilkSurf",
        version: env!("CARGO_PKG_VERSION"),
    },
    BrandVersion {
        brand: GREASE_BRAND,
        version: "8.0.0",
    },
];

/// Full SilkSurf version, the `uaFullVersion` high-entropy value.
pub const FULL_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Platform brand from the UA-CH `Sec-CH-UA-Platform` value list.
pub const PLATFORM: &str = if cfg!(target_os = "android") {
    "Android"
} else if cfg!(target_os = "ios") {
    "iOS"
} else if cfg!(target_os = "linux") {
    "Linux"
} else if cfg!(target_os = "macos") {
    "macOS"
} else if cfg!(target_os = "windows") {
    "Windows"
} else if cfg!(target_os = "fuchsia") {
    "Fuchsia"
} else {
    "Unknown"
};

/// Mobile form-factor preference.
pub const MOBILE: bool = cfg!(any(target_os = "android", target_os = "ios"));

/// CPU architecture family, the `architecture` high-entropy value.
pub const ARCHITECTURE: &str = if cfg!(any(target_arch = "x86", target_arch = "x86_64")) {
    "x86"
} else if cfg!(any(target_arch = "arm", target_arch = "aarch64")) {
    "arm"
} else {
    ""
};

/// Pointer width, the `bitness` high-entropy value.
pub const BITNESS: &str = if cfg!(target_pointer_width = "64") {
    "64"
} else {
    "32"
};

/// Form factor, the `formFactors` high-entropy value.
pub const FORM_FACTOR: &str = if MOBILE { "Mobile" } else { "Desktop" };

static SEC_CH_UA: LazyLock<String> = LazyLock::new(|| {
    BRANDS
        .iter()
        .map(|entry| format!("\"{}\";v=\"{}\"", entry.brand, entry.version))
        .collect::<Vec<_>>()
        .join(", ")
});

static SEC_CH_UA_PLATFORM: LazyLock<String> = LazyLock::new(|| format!("\"{PLATFORM}\""));

/// `Sec-CH-UA` structured-header list serialized from [`BRANDS`].
#[must_use]
pub fn sec_ch_ua() -> &'static str {
    SEC_CH_UA.as_str()
}

/// `Sec-CH-UA-Mobile` structured-header boolean.
#[must_use]
pub fn sec_ch_ua_mobile() -> &'static str {
    if MOBILE { "?1" } else { "?0" }
}

/// `Sec-CH-UA-Platform` structured-header string.
#[must_use]
pub fn sec_ch_ua_platform() -> &'static str {
    SEC_CH_UA_PLATFORM.as_str()
}

/// Low-entropy request headers in the Client Hints low entropy hint table.
#[must_use]
pub fn low_entropy_headers() -> [(&'static str, &'static str); 3] {
    [
        ("sec-ch-ua", sec_ch_ua()),
        ("sec-ch-ua-mobile", sec_ch_ua_mobile()),
        ("sec-ch-ua-platform", sec_ch_ua_platform()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sec_ch_ua_serializes_every_brand_including_grease() {
        assert_eq!(
            sec_ch_ua(),
            format!(
                "\"SilkSurf\";v=\"{}\", \"Not_A Brand\";v=\"8\"",
                env!("CARGO_PKG_VERSION_MAJOR")
            )
        );
        let grease = BRANDS[1].brand;
        assert!(grease.len() <= 20 && grease.contains(' '));
        assert_eq!(grease.trim(), grease);
        assert_eq!(BRANDS.len(), FULL_VERSION_LIST.len());
    }
}
