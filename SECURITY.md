# Security policy

## Reporting a vulnerability

Please report security problems privately through GitHub's [private vulnerability reporting](https://github.com/wapenshaw/ocb-inspector/security/advisories/new). Do not open a public issue for them. Include what you found, how to reproduce it and which version or commit you tested. You should get a first reply within a week.

Things worth reporting include:

- A crafted BIOS image or `.ocb` profile that crashes the app, makes it hang or use unbounded memory, or runs code.
- A way to make the app or CLI write a profile without the experimental writing switch, for an image that is not approved, or with settings other than the reviewed ones.
- A way for the desktop window to read or write files beyond the ones the user chose, or to reach the network.
- A file written outside the folder the user chose, or an input file overwritten.

## Supported versions

Only the latest release and the `master` branch receive fixes.

## Scope

A profile the app writes is loaded by the BIOS, which decides what to do with it. How a BIOS treats a profile, including one with unusual values, is not an OCB Inspector vulnerability. Writing is experimental and off by default for this reason.
