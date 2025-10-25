use regex::Regex;
use std::sync::LazyLock;

static API_TO_APP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(https?://)api([^/]*)(/api)?").unwrap());

/// Convert an API base URL to an app base URL.
///
/// Rules:
/// - https://api$ANYTHING/api -> https://app$ANYTHING
/// - https://api$ANYTHING -> https://app$ANYTHING (this is mainly for local dev @ api.nominal.test)
///
/// Examples:
/// - https://api.gov.nominal.io/api -> https://app.gov.nominal.io
/// - https://api-staging.gov.nominal.io/api -> https://app-staging.gov.nominal.io
/// - https://api.nominal.test -> https://app.nominal.test
pub fn api_base_url_to_app_base_url(api_base_url: &str) -> String {
    let api_base_url = api_base_url.trim_end_matches('/');
    if let Some(caps) = API_TO_APP_RE.captures(api_base_url) {
        return format!("{}{}{}", &caps[1], "app", &caps[2]);
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_app_url_conversion() {
        assert_eq!(
            api_base_url_to_app_base_url("https://api.gov.nominal.io/api"),
            "https://app.gov.nominal.io"
        );
        assert_eq!(
            api_base_url_to_app_base_url("https://api-staging.gov.nominal.io/api"),
            "https://app-staging.gov.nominal.io"
        );
        assert_eq!(
            api_base_url_to_app_base_url("https://api.nominal.test"),
            "https://app.nominal.test"
        );
        assert_eq!(
            api_base_url_to_app_base_url("https://api-customer.eu.nominal.io/api"),
            "https://app-customer.eu.nominal.io"
        );
        assert_eq!(
            api_base_url_to_app_base_url("https://api-customer.gov.nominal.io/api"),
            "https://app-customer.gov.nominal.io"
        );
        assert_eq!(
            api_base_url_to_app_base_url("https://api.nominal.gov.deployment.customer.com/api"),
            "https://app.nominal.gov.deployment.customer.com"
        );
        assert_eq!(
            api_base_url_to_app_base_url("https://api.nominal.customer.internal/api"),
            "https://app.nominal.customer.internal"
        );
        assert_eq!(api_base_url_to_app_base_url("https://unknown"), "");
    }
}
