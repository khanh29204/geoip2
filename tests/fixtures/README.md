# MaxMind-DB test fixtures

These unmodified test databases are from the official MaxMind-DB repository:

- `GeoIP2-City-Test.mmdb`
- `GeoLite2-City-Test.mmdb`
- `GeoIP2-Country-Test.mmdb` (wrong-type validation)
- `GeoIP2-City-Test-Broken-Double-Format.mmdb` (lookup decode error)
- `GeoLite2-ASN-Test.mmdb` (ASN records)

Source: <https://github.com/maxmind/MaxMind-DB/tree/d1551916cff0712d12ff602c309234471b0402a2/test-data>
Pinned repository revision: `d1551916cff0712d12ff602c309234471b0402a2`.

Copyright (c) 2013-2026 MaxMind, Inc. MaxMind-DB is made available under
Apache License 2.0 or MIT License, at your option. The upstream license texts
are included as `LICENSE-APACHE` and `LICENSE-MIT` in this directory.

The `89.160.20.128/25` expected values asserted by the tests were independently
checked against the upstream `source-data/GeoIP2-City-Test.json` and
`source-data/GeoLite2-City-Test.json` at the same revision. ASN expectations
`89.160.20.128` → `29518` / `Bredband2 AB` and `1.0.0.1` → `15169` / `Google Inc.`
were checked against `source-data/GeoLite2-ASN-Test.json` at that revision.
