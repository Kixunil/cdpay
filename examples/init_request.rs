extern crate cdpay;
extern crate tokio_core;

fn main() {
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

    let result = core.run(builder.init_payment_request(&data).unwrap());
    println!("Payment result: {:?}", result);
}
