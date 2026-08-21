# Security policy

## Read this before you rely on EuleDB for anything that matters

EuleDB is in early development and it ships cryptography — AES-256-GCM for data at rest, Argon2id for
key derivation. That combination deserves a plain statement rather than a reassuring one:

- **The cryptographic design has not been independently audited.** It uses audited primitives from
  well-reviewed crates, which is not the same thing as the composition being reviewed.
- **There has been no release yet.** Nothing has been exposed to real use, real data or real attackers.
- **Do not use it as the only protection for data whose disclosure would harm someone**, until this
  section says otherwise.

That is not modesty. Encryption that is trusted more than it has earned is worse than no encryption,
because it changes what people are willing to store.

## Supported versions

| Version | Supported |
|---|---|
| latest release | yes |
| anything older | no |

Below 1.0.0 there are no maintenance branches and no backports. A fix ships in the next release, and
the upgrade path is to take it. That is a consequence of one maintainer, stated rather than discovered.

## Reporting a vulnerability

**Use [private reporting](https://github.com/twohreichel/EuleDB/security/advisories/new). Never open a
public issue for a vulnerability.**

Include what you would want to receive: what you did, what happened, what you expected, and the version
or commit. A reproduction — a failing test, a file, a sequence of calls — is worth more than a
description of impact.

What to expect in return:

- An acknowledgement that a human has read it. **Best effort, no service level.** One maintainer cannot
  honestly promise a response within a fixed number of hours, so no number is promised here.
- A decision on whether it is in scope, with reasons if it is not.
- Credit in the advisory and the release notes, unless you would rather not be named.

There is no bug bounty and no payment. Saying so up front is more useful than leaving it open.

## What counts

**In scope** — anything that breaks a property the software claims:

- Recovering plaintext, or any part of it, without the passphrase.
- A wrong passphrase or a failed authentication tag yielding data instead of an error.
- Reading or writing past the scope an access grant permits.
- Reaching code execution, a file write, or a network call through data that was merely stored or queried.
- A crash, hang or unbounded allocation triggered by untrusted input — an embedded library that can be
  made to take down its host process is a real problem, not a robustness nit.
- Tampering with the audit trail without detection.
- A secret, key or passphrase reaching a log, an error message or a temporary file.

**Out of scope** — real problems, but not ones this project can fix:

- An attacker who already has your passphrase, your process memory, or root on your machine.
- Weaknesses in the primitives themselves. Report those upstream.
- Findings from an automated scanner with no demonstrated impact here.
- Denial of service by supplying a legitimately enormous workload.

## Fix handling

A confirmed vulnerability is fixed on a private fork, released, and disclosed in a
[GitHub Security Advisory](https://github.com/twohreichel/EuleDB/security/advisories) with a CVE where
one applies. The advisory is published when the fix is available, not before — and it is published even
when nobody appears to have been affected, because a silent fix denies everyone else the chance to
notice they were.
