# Security Policy

Please report security issues privately to the repository owner instead of opening a public issue.

The following are security-sensitive:

- update signature verification or path confinement bypasses;
- arbitrary file write, DLL loading or command execution;
- prompt-injection paths that escape the evidence-only answer contract;
- exposure of the Ed25519 release private key;
- untrusted remote content being opened inside the desktop webview.

If the update signing key may have been exposed, stop publishing updates immediately. Existing clients trust the public key embedded in their installer, so key rotation requires a separately authenticated application release.
