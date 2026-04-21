use futures::{
    StreamExt,
    stream::{self, Stream},
};
use nominal_api::objects::api::Token;
use regex::Regex;
use std::sync::Arc;
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
pub(crate) fn api_base_url_to_app_base_url(api_base_url: &str) -> String {
    let api_base_url = api_base_url.trim_end_matches('/');
    if let Some(caps) = API_TO_APP_RE.captures(api_base_url) {
        return format!("{}{}{}", &caps[1], "app", &caps[2]);
    }
    String::new()
}

/// Drives a paginated API call lazily, yielding items one by one across all pages.
///
/// All closures must be `'static + Send` so the returned stream can be sent across threads.
/// In practice this means cloning any borrowed data (e.g. service clients, tokens) before
/// passing closures in.
///
/// - `make_request`: builds the request for each page given the current page token.
/// - `call`: performs the async RPC.
/// - `next_token`: extracts the next page token from a response (`None` = last page).
/// - `into_items`: converts a response into its item vec.
pub(crate) fn paginate_stream<Req, Resp, Item, MakeReq, Call, CallFut, NextToken, IntoItems>(
    make_request: MakeReq,
    call: Call,
    next_token: NextToken,
    into_items: IntoItems,
) -> impl Stream<Item = crate::Result<Item>>
where
    MakeReq: Fn(Option<Token>) -> Req + 'static,
    Call: Fn(Req) -> CallFut + 'static,
    CallFut: std::future::Future<Output = crate::Result<Resp>>,
    NextToken: Fn(&Resp) -> Option<Token> + 'static,
    IntoItems: Fn(Resp) -> Vec<Item> + 'static,
{
    let make_request = Arc::new(make_request);
    let call = Arc::new(call);
    let next_token = Arc::new(next_token);
    let into_items = Arc::new(into_items);

    stream::unfold(Some(None::<Token>), move |state| {
        let make_request = Arc::clone(&make_request);
        let call = Arc::clone(&call);
        let next_token = Arc::clone(&next_token);
        let into_items = Arc::clone(&into_items);
        async move {
            let page_token = state?;
            let req = make_request(page_token);
            match call(req).await {
                Err(e) => Some((vec![Err(e)], None)),
                Ok(resp) => {
                    let next = next_token(&resp).map(Some);
                    let items = into_items(resp).into_iter().map(Ok).collect();
                    Some((items, next))
                }
            }
        }
    })
    .flat_map(stream::iter)
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
