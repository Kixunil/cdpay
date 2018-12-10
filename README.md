CryptoDiggers payment gateway
=============================

Rust implementation of [CryptoDiggers](https://cryptodiggers.eu/) payment API.

About
-----

This crate provides an implementation of CryptoDiggers payment API version 1.
The implementation is async, using futures + tokio. It supports several
cryptocurrencies, including test networks and several fiat currencies.

The interface is strongly typed to avoid problems, since this is important
for security,

Disclaimer
----------

The author of this crate doesn't provide any guarantees when it comes to
correctness, security or any other property that might be important for not
losing money. The users of this library are wholy responsible for reviewing
the code and using it. For further information, see the MITNFA license.

The author of this crate reserves the right to **publicly** ridicule any person
or company experiencing any problems with incorrect use of this crate (where 
not reviewing it or not contracting independednt third party reviewer is
considered incorrect use).

Do **not** use this software if you are in fear of being ridiculed!

License
-------

MITNFA
