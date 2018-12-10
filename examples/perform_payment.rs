extern crate cdpay;
extern crate tokio_core;
extern crate futures;
extern crate tokio_retry;

fn main() {
    use futures::Future;

    let mut core = tokio_core::reactor::Core::new().unwrap();
    let builder = cdpay::CDPayBuilder::new_test(core.handle(), "cc97ef07-0654-11e6-88cc-00155d00c800".into());
    let data = cdpay::RequestData {
        timeout: cdpay::Timeout::new(10).unwrap(),
        order_id: cdpay::OrderID::new("12345".into()).unwrap(),
        amount: 1337,
        fiat_currency: cdpay::FiatCurrency::Eur,
        crypto_currency: cdpay::CryptoCurrency::BTC,
        wait_for_confirmantions: false,
    };

    let request = builder.init_payment_request(&data).unwrap()
        .map_err(cdpay::PaymentError::from)
        .map_err(tokio_retry::Error::OperationError)
        .and_then(|info| {
            println!("Payment initialized. Send {} coins to {}", info.amount, info.address);
            println!("Awaiting transaction");

            info.handle.wait_payment()
    });

    let result = core.run(request);
    match result {
        Ok(_) => {
            println!("Payment received.");
        },
        Err(err) => {
            println!("Error: {:?}", err);
        }
    }
}
