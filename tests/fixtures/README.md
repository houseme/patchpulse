# TLS Contract Fixtures

localhost-cert.der and localhost-key.der are generated self-signed test material.
The private key is public by design and is used only by a loopback test server to
verify that the production HTTPS client rejects an untrusted certificate. The
production image does not copy these files.
