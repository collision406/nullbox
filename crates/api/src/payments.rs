//! Payments through Stripe Checkout, in test mode only.
//!
//! The server decides what is sold and for how much; the browser only receives the URL of
//! a Stripe-hosted page, so no card number and no price decision ever passes through here.

use std::time::Duration;

use reqwest::Client;
use reqwest::redirect::Policy;
use serde::Deserialize;

pub const STRIPE_API_BASE: &str = "https://api.stripe.com";
pub const PRODUCT_NAME: &str = "Feed the Backlog Beast";
pub const PRODUCT_DESCRIPTION: &str =
    "One snack for the Backlog Beast. It was eaten before you could ask for a receipt.";
/// JPY has no minor unit: 500 means ¥500.
pub const PRICE_JPY: u32 = 500;
/// Stripe's tax code for "General - Electronically Supplied Services". Managed Payments, which
/// is on by default for the account, refuses a line item without a product tax code.
const TAX_CODE: &str = "txcd_10000000";
/// A checkout URL that doesn't start like this is never passed on to a browser.
const CHECKOUT_URL_PREFIX: &str = "https://checkout.stripe.com/";

#[derive(Debug, thiserror::Error)]
pub enum PaymentError {
    #[error("could not reach Stripe: {0}")]
    Unreachable(String),
    #[error("Stripe refused the request (HTTP {status}): {message}")]
    Refused { status: u16, message: String },
    #[error("unexpected answer from Stripe: {0}")]
    Unexpected(String),
}

pub struct Stripe {
    http: Client,
    base: String,
    api_key: String,
}

/// A Checkout Session that was just opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutSession {
    pub id: String,
    pub url: String,
}

#[derive(Deserialize)]
struct SessionBody {
    id: String,
    url: Option<String>,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: Option<ErrorDetail>,
}

#[derive(Deserialize)]
struct ErrorDetail {
    message: Option<String>,
}

impl Stripe {
    pub fn new(base: &str, api_key: &str) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            // The key travels in a header: never follow a redirect to somewhere else.
            .redirect(Policy::none())
            .build()
            .expect("the TLS backend initialises");
        Self { http, base: base.trim_end_matches('/').to_owned(), api_key: api_key.to_owned() }
    }

    /// Opens a one-off checkout for the Backlog Beast's snack. Paying returns the buyer to
    /// `{public_web_url}/thanks`, cancelling to `{public_web_url}/buy`.
    pub async fn create_checkout(&self, public_web_url: &str) -> Result<CheckoutSession, PaymentError> {
        // Stripe fills in the {CHECKOUT_SESSION_ID} placeholder itself.
        let success_url = format!("{public_web_url}/thanks?session_id={{CHECKOUT_SESSION_ID}}");
        let cancel_url = format!("{public_web_url}/buy");
        let price = PRICE_JPY.to_string();
        let form = [
            ("mode", "payment"),
            ("success_url", success_url.as_str()),
            ("cancel_url", cancel_url.as_str()),
            ("line_items[0][quantity]", "1"),
            ("line_items[0][price_data][currency]", "jpy"),
            ("line_items[0][price_data][unit_amount]", price.as_str()),
            ("line_items[0][price_data][product_data][name]", PRODUCT_NAME),
            ("line_items[0][price_data][product_data][description]", PRODUCT_DESCRIPTION),
            ("line_items[0][price_data][product_data][tax_code]", TAX_CODE),
            ("metadata[app]", "nullbox"),
        ];

        let unreachable = |error: reqwest::Error| PaymentError::Unreachable(error.without_url().to_string());
        let response = self
            .http
            .post(format!("{}/v1/checkout/sessions", self.base))
            .bearer_auth(&self.api_key)
            .form(&form)
            .send()
            .await
            .map_err(unreachable)?;
        let status = response.status();
        let body = response.text().await.map_err(unreachable)?;

        if !status.is_success() {
            let message = serde_json::from_str::<ErrorBody>(&body)
                .ok()
                .and_then(|parsed| parsed.error)
                .and_then(|error| error.message)
                .unwrap_or_else(|| "no error message".to_owned());
            return Err(PaymentError::Refused { status: status.as_u16(), message });
        }
        let session: SessionBody =
            serde_json::from_str(&body).map_err(|error| PaymentError::Unexpected(error.to_string()))?;
        match session.url {
            Some(url) if url.starts_with(CHECKOUT_URL_PREFIX) => Ok(CheckoutSession { id: session.id, url }),
            _ => Err(PaymentError::Unexpected("the session has no Stripe checkout URL".to_owned())),
        }
    }
}
