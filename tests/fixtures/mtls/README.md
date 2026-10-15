These certificates and private keys are public, test-only fixtures. Never use them
in production. Encrypted keys use `probe-test-only-password`. The CA private key
is not retained. RSA keys use PKCS#1, the EC key uses SEC1, and the PKCS#8 variants
derive from the same RSA client key. The wrong client is self-signed and untrusted.

The current server certificate expires **2027-10-14**. Regenerate the fixtures
before that date (reminder: September 2027) so macOS coverage and mTLS tests keep
passing. Its 365-day validity, SANs (`localhost`, `127.0.0.1`), `serverAuth` EKU,
and critical `CA:FALSE` basic constraint satisfy macOS leaf verification. Other
fixtures expire in 2126.

From the repository root, with Python 3 and OpenSSL 3 installed, run exactly:

```sh
python3 tests/fixtures/mtls/regenerate.py
openssl x509 -in tests/fixtures/mtls/server.pem -noout -dates -ext subjectAltName
cargo test --locked -p probe-http -p probe-application -p probe-cli
```

[regenerate.py](regenerate.py) contains the exact OpenSSL invocations. It regenerates
the complete fixture set together: CA, RSA/EC clients, untrusted client, server,
PKCS#8 and encrypted variants, and leaf-first chain. It uses a temporary directory
for the CA key, CSRs and extensions and removes it on exit. No CA private key is
published. After regeneration, update this expiry reminder to the new server
expiry and commit all changed fixture PEM files together. OpenSSL is fixture
maintenance tooling; Probe's production identity loader uses Rust libraries.

PKCS#12 fixtures reuse these public test keys and certificates. Run
`python3 tests/fixtures/mtls/regenerate-pkcs12.py` after regenerating PEMs (or on its
own to leave PEMs unchanged). That script records every OpenSSL 3 invocation and
constructs special unencrypted archives with small Python DER helpers: mismatched
keys, duplicate keys/certificates, reversed chain order, certificate/key-only stores,
multiple distinct identities, and an unsupported nested bag. OpenSSL exports cover
AES-256/SHA-256, AES-128/SHA-1, empty passwords, encrypted MAC-less content and
unsupported legacy PBE. All nonempty passwords are `probe-test-only-password`.
OpenSSL is used only to generate fixtures; parsing/decryption tests use RustCrypto.
