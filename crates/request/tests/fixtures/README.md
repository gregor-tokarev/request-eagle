`localhost.pem` and `localhost-key.pem` are a self-signed certificate and its
publicly committed test-only private key. They are used only by loopback test
servers to check the certificate verification preference. Never use them outside
tests.

Generated with:

```sh
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 \
  -keyout localhost-key.pem -out localhost.pem -sha256 -days 36500 -nodes \
  -subj '/CN=localhost' -addext 'subjectAltName=DNS:localhost,IP:127.0.0.1'
```
