//! This crate provides a client implementation of [CryptoDigger](https://cryptodiggers.eu/)s payment gateway API version 1.
//!
//! The API is implemented as async, using Futures+Tokio.
//!
//! The interface is strongly typed to avoid problems, since this is important
//! In order to use this crate, you must instantiate `CDPayBuilder` which is then used to create
//! paymet requests - see its documentation.
//! 
//! Disclaimer
//! ----------
//! 
//! The author of this crate doesn't provide any guarantees when it comes to
//! correctness, security or any other property that might be important for not
//! losing money. The users of this library are wholy responsible for reviewing
//! the code and using it. For further information, see the MITNFA license.
//! 
//! The author of this crate reserves the right to **publicly** ridicule any person
//! or company experiencing any problems with incorrect use of this crate (where 
//! not reviewing it or not contracting independednt third party reviewer is
//! considered incorrect use).
//! 
//! Do **not** use this software if you are in fear of being ridiculed!

extern crate futures;
extern crate hyper;
extern crate hyper_tls;
extern crate native_tls;
extern crate tokio_core;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate urlencoding;
extern crate tokio_retry;

#[cfg(feature = "slog")]
#[macro_use]
extern crate slog;

#[macro_use]
mod logging;

use tokio_core::reactor::Handle;
use hyper::Client;
use std::fmt;
use futures::Future;
use futures::Stream;
use futures::future::Either;
use futures::Poll;
use hyper::StatusCode;
use hyper::header::ContentLength;
use hyper_tls::HttpsConnector;
use native_tls::Error as TlsError;
use urlencoding::encode as url_encode;
use tokio_retry::RetryIf;
pub use tokio_retry::Error as RetryError;

use logging::Logger;

/// Information about payment
#[derive(Debug)]
pub struct PaymentInfo {
    /// The address to pay to.
    pub address: String,
    /// The amount of currency - as string to avoid problems with rounding.
    pub amount: String,
    /// The selected crypto currency.
    pub currency_id: CryptoCurrency,
    /// The selected crypto currency as string.
    pub currency: String,
    /// Message
    pub msg: String,
    /// Payent handle.
    ///
    /// Allows waiting for payment or getting iframe ID.
    pub handle: PaymentHandle,
}

impl PaymentInfo {
    fn new(internal: PaymentInfoInternal, handle: Handle, is_testnet: bool, logger: Logger) -> Self {
        let logger = logging::logger_with_iframe_id(&logger, &internal.iframe_id);
        PaymentInfo {
            address: internal.address,
            amount: internal.amount,
            currency_id: internal.currency_id,
            currency: internal.currency,
            msg: internal.msg,
            handle: PaymentHandle {
                iframe_id: internal.iframe_id,
                handle,
                is_testnet,
                logger,
            }
        }
    }
}

struct PaymentErrorCondition;

impl<T> tokio_retry::Condition<Option<T>> for PaymentErrorCondition {
    fn should_retry(&mut self, error: &Option<T>) -> bool {
        error.is_none()
    }
}

#[derive(Deserialize)]
struct StatusResponse {
    error: u64,
    error_msg: String,
    status_msg: Option<String>,
    status_id: u64,
}

struct PaymentHandleInternal {
    iframe_id: String,
    is_testnet: bool,
    handle: Handle,
    logger: Logger,
}

impl tokio_retry::Action for PaymentHandleInternal {
    type Future = Box<Future<Item=Self::Item, Error=Self::Error>>;
    type Item = ();
    type Error = Option<PaymentError>;

    fn run(&mut self) -> Self::Future {
        use futures::IntoFuture;

        let url = format!("https://www.cryptodiggers{}.eu/api/api.php?iframe={}&a=get_iframe_status", if self.is_testnet { "test" } else { "" }, url_encode(&self.iframe_id));
        debug!(self.logger, "Querying status"; "url" => &url);
        let logger = self.logger.clone();
        let url = url.parse().unwrap();
        let https_connector = HttpsConnector::new(4, &self.handle);
        let https_connector = match https_connector {
            Ok(connector) => connector,
            Err(err) => return Box::new(Err(err).map_err(APIError::from).map_err(PaymentError::from).map_err(Some).into_future()) as Self::Future,
        };
        let client = Client::configure()
            .connector(https_connector)
            .build(&self.handle);
        Box::new(client
            .get(url)
            .map_err(Into::into)
            .and_then(move |res| {
                if res.status() == StatusCode::Ok {
                    debug!(logger, "Status: 200 OK");
                    let vec = res.headers().get::<ContentLength>().and_then(|len| if len.0 < 1_000_000 { Some(Vec::with_capacity(len.0 as usize)) } else { None }).unwrap_or_else(Vec::new);
                    // TODO: Limit
                    Either::A(res.body()
                        .map_err(APIError::from)
                        .fold(vec, |mut vec, chunk| -> Result<_, APIError> { vec.extend_from_slice(&chunk); Ok(vec) })
                        .and_then(move |vec| {
                            let mut reader = &vec as &[u8];
                            // Hook to remove PHP dump
                            // TODO: Remove
                            if reader.len() > 5 {
                                if &reader[..5] == b"Array" {
                                    let pos = reader.iter().take_while(|c| **c != b'{').count();
                                    let tmp = std::mem::replace(&mut reader, &[]);
                                    let (_, tmp) = tmp.split_at(pos);
                                    reader = tmp;
                                }
                            }
                            // END OF Hook to remove PHP dump

                            #[cfg(feature = "slog")]
                            std::str::from_utf8(&reader)
                                .map(|s| debug!(logger, "Received response"; "response_data" => s))
                                .unwrap_or_else(|error| error!(logger, "Failed to parse response as UTF-8"; "error" => %error));

                            let response: StatusResponse = serde_json::from_reader(&mut reader)
                                .map_err(|error| {
                                    error!(logger,"Failed to parse response"; "error" => %error);
                                    error
                                })?;
                            
                            Ok(response)
                        }))
                } else {
                    error!(logger, "HTTP request failed."; "status" => %res.status());
                    Either::B(Err(APIError::UnexpectedStatus(res.status())).into_future())
                }
            })
            .map_err(PaymentError::from)
            .map_err(Some)
            .and_then(Result::from)
            ) as Self::Future
    }
}

/// Future that resolves when the request is paid.
pub struct WaitPayment {
    checker: RetryIf<tokio_retry::strategy::FixedInterval, PaymentHandleInternal, PaymentErrorCondition>,
}

impl Future for WaitPayment {
    type Item = ();
    type Error = RetryError<PaymentError>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.checker.poll().map_err(|error| match error {
            RetryError::OperationError(error) => RetryError::OperationError(error.unwrap()),
            RetryError::TimerError(error) => RetryError::TimerError(error),
        })
    }
}

/// A payment identifier that can be used to wait for payment.
#[derive(Debug)]
pub struct PaymentHandle {
    iframe_id: String,
    is_testnet: bool,
    handle: Handle,
    logger: Logger,
}

impl PaymentHandle {
    /// Returns iframe ID associated with the payment.
    pub fn get_iframe_id(&self) -> &str {
        &self.iframe_id
    }

    /// Turns this handle into a future that resolves when
    /// the payment is finished.
    pub fn wait_payment(self) -> WaitPayment {
        use std::time::Duration;

        let internal_handle = PaymentHandleInternal {
            iframe_id: self.iframe_id,
            is_testnet: self.is_testnet,
            handle: self.handle.clone(),
            logger: self.logger,
        };

        let strategy = tokio_retry::strategy::FixedInterval::new(Duration::from_secs(5));

        WaitPayment {
            checker: RetryIf::spawn(self.handle, strategy, internal_handle, PaymentErrorCondition),
        }
    }
}

#[derive(Debug, Deserialize)]
struct PaymentInfoInternal {
    #[serde(rename = "address_value_out")]
    pub address: String,

    #[serde(rename = "amount_out")]
    pub amount: String,

    pub iframe_id: String,
    #[serde(rename = "currency_id_out")]
    pub currency_id: CryptoCurrency,
    #[serde(rename = "currency_out")]
    pub currency: String,

    #[serde(rename = "Msg")]
    pub msg: String,
}

#[derive(Deserialize)]
struct InitResponse {
    address: Vec<PaymentInfoInternal>,
    error: u64,
    error_msg: String,
}

/// Error that might happen when communicating with payment API.
#[derive(Debug)]
pub enum APIError {
    MissingData,
    /// Error returned by the server as specified by the API.
    Error {
        code: u64,
        message: String,
    },
    /// Parsing the responnse as json failed.
    Deserialization(serde_json::Error),
    /// Underlying HTTP communication failed.
    Http(hyper::error::Error),
    /// HTTP protocol returned unexpected status.
    UnexpectedStatus(StatusCode),
    /// TLS communication failed.
    Tls(TlsError),
}

/// The ways payment might fail
#[derive(Debug)]
pub enum PaymentError {
    /// The amount of cryptocurrency sent to the address wasn't correct.
    IncorrectAmount,
    /// The payment wasn't received.
    NotReceived,
    /// Communication failed
    Communication(APIError),
    /// Other error returned by te server.
    Other { status_id: u64, status_message: Option<String> },
}

impl From<StatusResponse> for Result<(), Option<PaymentError>> {
    fn from(response: StatusResponse) -> Self {
        if response.error == 0 {
            match response.status_id {
                1 => Ok(()),
                2 => Err(Some(PaymentError::IncorrectAmount)),
                4 => Err(None),
                5 => Err(Some(PaymentError::NotReceived)),
                _ => Err(Some(PaymentError::Other { status_id: response.status_id, status_message: response.status_msg })),
            }
        } else {
            Err(Some(PaymentError::Communication(APIError::Error { code: response.error, message: response.error_msg })))
        }
    }
}

impl From<APIError> for PaymentError {
    fn from(error: APIError) -> Self {
        PaymentError::Communication(error)
    }
}

impl From<serde_json::Error> for APIError {
    fn from(error: serde_json::Error) -> Self {
        APIError::Deserialization(error)
    }
}

impl From<hyper::error::Error> for APIError {
    fn from(error: hyper::error::Error) -> Self {
        APIError::Http(error)
    }
}

impl From<TlsError> for APIError {
    fn from(error: TlsError) -> Self {
        APIError::Tls(error)
    }
}

/// Timeout accepted by the server.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Timeout (u8);

impl Timeout {
    /// Attempts to instantiate timeout value.
    ///
    /// The parameter `timeout` is number of minutes to wait.
    /// The timeout must be at least 5 min and at most 30 min.
    pub fn new(timeout: u8) -> Option<Self> {
        if timeout >= 10 && timeout <= 30 {
            Some(Timeout(timeout))
        } else {
            None
        }
    }
}

impl From<Timeout> for u8 {
    fn from(timeout: Timeout) -> Self {
        timeout.0
    }
}

/// User-defined order ID.
///
/// This is a newtype in order to avoid potential mistakes
/// and make the code more clear.
pub struct OrderID (String);

impl OrderID {
    /// Creates `OrderID` from given string
    pub fn new(order_id: String) -> Option<Self> {
        if order_id.len() <= 20 && order_id.chars().all(|c| (c >= '0' && c <= '9') || (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '-' || c == '_') {
            Some(OrderID(order_id))
        } else {
            None
        }
    }
}

impl From<OrderID> for String {
    fn from(order_id: OrderID) -> Self {
        order_id.0
    }
}

impl<'a> From<&'a OrderID> for &'a str {
    fn from(order_id: &OrderID) -> &str {
        &order_id.0
    }
}

impl<'a> AsRef<str> for OrderID {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

macro_rules! enum_number {
    ($name:ident { $($variant:ident = $value:expr, )* }) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum $name {
            $($variant = $value,)*
        }

        impl ::serde::Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
                where S: ::serde::Serializer
            {
                // Serialize the enum as a u64.
                serializer.serialize_u64(*self as u64)
            }
        }

        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                where D: ::serde::Deserializer<'de>
            {
                struct Visitor;

                impl<'de> ::serde::de::Visitor<'de> for Visitor {
                    type Value = $name;

                    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                        formatter.write_str("positive integer")
                    }

                    fn visit_u64<E>(self, value: u64) -> Result<$name, E>
                        where E: ::serde::de::Error
                    {
                        // Rust does not come with a simple way of converting a
                        // number to an enum, so use a big `match`.
                        match value {
                            $( $value => Ok($name::$variant), )*
                            _ => Err(E::custom(
                                format!("unknown {} value: {}",
                                stringify!($name), value))),
                        }
                    }
                }

                // Deserialize the enum from a u64.
                deserializer.deserialize_u64(Visitor)
            }
        }
    }
}

enum_number!(FiatCurrency {
    Eur = 1,
    USD = 2,
    GBP = 3,
    CAD = 4,
    AUD = 5,
    JPY = 9,
    CNY = 13,
    CZK = 16,
    AED = 17,
});

enum_number!(CryptoCurrency {
    BTC = 6,
    WDC = 7,
    LTC = 8,
    Dash = 19,
});

type InitRequest = Box<Future<Item=PaymentInfo, Error=APIError>>;

/// Contains all information required to initiate the payment.
pub struct RequestData {
    /// How long the request is valid.
    pub timeout: Timeout,
    /// User-selected order ID.
    pub order_id: OrderID,
    /// The amount of fiat currency to be paid **in cents**.
    pub amount: u64,
    /// The fiat currency to use to specify amount.
    pub fiat_currency: FiatCurrency,
    /// The crypto currency a user wants to use for payment.
    pub crypto_currency: CryptoCurrency,
    /// Selects whether to wait for blockchain confirmations.
    ///
    /// In order to not risk too much, you should set this to `true`,
    /// if the amount is large.
    pub wait_for_confirmantions: bool,
}

/// The payment gateway.
///
/// This struct contains all necessary information used to connect to
/// the payment gateway.
pub struct CDPayBuilder {
    handle: Handle,
    api_key: String,
    is_testnet: bool,
    logger: logging::Logger,
}

impl CDPayBuilder {
    /// Initiates payment builder.
    pub fn new(tokio_handle: Handle, api_key: String) -> Self {
        CDPayBuilder {
            handle: tokio_handle,
            api_key,
            is_testnet: false,
            logger: logging::default_logger(),
        }
    }

    /// Initiates payment builder using **test network**.
    ///
    /// **Warning**: This doesn't use real coins - only test coins. It is
    /// guaranteed that using this in exchange for real products will lead
    /// to loss!
    pub fn new_test(tokio_handle: Handle, api_key: String) -> Self {
        CDPayBuilder {
            handle: tokio_handle,
            api_key,
            is_testnet: true,
            logger: logging::default_logger(),
        }
    }

    /// Sets the logger for this instance.
    ///
    /// This function creates a child logger that logs network kind.
    ///
    /// Only available if `slog` feature is enabled.
    #[cfg(feature = "slog")]
    pub fn set_logger(&mut self, logger: &slog::Logger) {
        let network = if self.is_testnet {
            "testnet"
        } else {
            "mainnet"
        };

        self.logger = logger.new(o!("network" => network, "api_key" => self.api_key.clone()));
    }

    /// Requests new payment.
    pub fn init_payment_request(&self, request_data: &RequestData) -> Result<InitRequest, TlsError> {
        use futures::IntoFuture;

        let url = format!("https://www.cryptodiggers{}.eu/api/api.php?apikey={}&a=new_address&timeout={}&order_id={}&amount={}.{}&currency={}&currency_crypto={}&wait={}",
                         if self.is_testnet { "test" } else { "" }, url_encode(&self.api_key), u8::from(request_data.timeout), request_data.order_id.as_ref(),
                         request_data.amount / 100, request_data.amount % 100, request_data.fiat_currency as u8, request_data.crypto_currency as u8,
                         request_data.wait_for_confirmantions as u8);

        let logger = logging::order_logger(&self.logger, &request_data.order_id);
        info!(logger, "Initiating payment"; "url" => &url, "timeout" => ?request_data.timeout, "amount" => request_data.amount, "fiat_currency" => ?request_data.fiat_currency, "crypto_currency" => ?request_data.crypto_currency, "wait_for_confirmations" => %request_data.wait_for_confirmantions);
        let url = url.parse().unwrap();
        let client = Client::configure()
            .connector(HttpsConnector::new(4, &self.handle)?)
            .build(&self.handle);

        let is_testnet = self.is_testnet;
        let handle = self.handle.clone();

        Ok(Box::new(client
            .get(url)
            .map_err(Into::into)
            .and_then(move |res| {
                if res.status() == StatusCode::Ok {
                    debug!(logger, "Status: 200 OK");

                    let vec = res.headers().get::<ContentLength>().and_then(|len| if len.0 < 1_000_000 { Some(Vec::with_capacity(len.0 as usize)) } else { None }).unwrap_or_else(Vec::new);
                    // TODO: Limit
                    Either::A(res.body()
                        .map_err(APIError::from)
                        .fold(vec, |mut vec, chunk| -> Result<_, APIError> { vec.extend_from_slice(&chunk); Ok(vec) })
                        .and_then(move |vec| {
                            let mut reader = &vec as &[u8];
                            #[cfg(feature = "slog")]
                            std::str::from_utf8(&reader)
                                .map(|s| debug!(logger, "Received response"; "response_data" => s))
                                .unwrap_or_else(|error| error!(logger, "Failed to parse response as UTF-8"; "error" => %error));
                            let mut response: InitResponse = serde_json::from_reader(&mut reader)
                                .map_err(|error| {
                                    error!(logger,"Failed to parse response"; "error" => %error);
                                    error
                                })?;
                            
                            if response.error == 0 {
                                response.address.pop().map(|internal| PaymentInfo::new(internal, handle, is_testnet, logger)).ok_or(APIError::MissingData)
                            } else {
                                error!(logger,"API call failed"; "error_code" => response.error, "error_message" => &response.error_msg);
                                Err(APIError::Error { code: response.error, message: response.error_msg })
                            }
                        }))
                } else {
                    Either::B(Err(APIError::UnexpectedStatus(res.status())).into_future())
                }
            })) as InitRequest)
    }
}
