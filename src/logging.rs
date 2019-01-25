///! Logging implementations
///!
///! This code aims to keep zero runtime cost of the logging infrastructure.
///! It acomplishes it by using empty struct when logging isn't enabled.

pub use self::impls::*;

#[cfg(feature = "slog")]
mod impls {
    use ::OrderID;

    pub use ::slog::Logger;

    pub fn default_logger() -> Logger {
        ::slog::Logger::root(::slog::Discard, o!())
    }

    pub fn order_logger(logger: &Logger, order_id: &OrderID) -> Logger {
        logger.new(o!("order_id" => String::from(order_id.as_ref())))
    }

    pub fn logger_with_iframe_id(logger: &Logger, iframe_id: &str) -> Logger {
        logger.new(o!("iframe_id" => String::from(iframe_id)))
    }
}

#[cfg(not(feature = "slog"))]
#[macro_use]
mod impls {
    use ::OrderID;
    use std::fmt;

    #[derive(Clone)]
    pub struct Logger;

    impl fmt::Debug for Logger {
        fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
           write!(f, "discarding logger")
        }
    }

    pub fn default_logger() -> Logger {
        Logger
    }

    pub fn order_logger(_logger: &Logger, _order_id: &OrderID) -> Logger {
        Logger
    }

    pub fn logger_with_iframe_id(_logger: &Logger, _iframe_id: &str) -> Logger {
        Logger
    }

    // Replacement macros for cases with logging turned off.
    // Allowing unused for future use
    #[allow(unused)]
    macro_rules! debug {
        ($logger:expr, #$tag:expr, $($args:tt)+) => {
            { let _ = $logger; }
        };

        ($logger:expr, $($args:tt)+) => {
            { let _ = $logger; }
        };
    }

    #[allow(unused)]
    macro_rules! info {
        ($logger:expr, #$tag:expr, $($args:tt)+) => {
            { let _ = $logger; }
        };

        ($logger:expr, $($args:tt)+) => {
            { let _ = $logger; }
        };
    }

    #[allow(unused)]
    macro_rules! warn {
        ($logger:expr, #$tag:expr, $($args:tt)+) => {
            { let _ = $logger; }
        };

        ($logger:expr, $($args:tt)+) => {
            { let _ = $logger; }
        };
    }

    #[allow(unused)]
    macro_rules! error {
        ($logger:expr, #$tag:expr, $($args:tt)+) => {
            { let _ = $logger; }
        };

        ($logger:expr, $($args:tt)+) => {
            { let _ = $logger; }
        };
    }
}
