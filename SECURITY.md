# Security Policy

## Supported versions

OxiSound follows the COOLJAPAN ecosystem's rolling-release model. Only
the **latest released 0.x line** is supported with security fixes.
Older 0.x releases do not receive backported patches; please upgrade
to the latest release before reporting an issue to confirm it is
still reproducible.

## Reporting a vulnerability

Please **do not** file a public GitHub issue for a suspected security
vulnerability. Instead, report it privately by emailing:

**info@kitasan.io**

Include as much detail as you can: affected crate and version, a
minimal reproduction (code or test case), the observed impact, and —
if known — a suggested fix or mitigation.

Reports are triaged privately by the maintainer. We will acknowledge
receipt as soon as practical, investigate, and coordinate a fix and
disclosure timeline with the reporter before any public disclosure.

## Scope

This policy covers the OxiSound crates published from this repository
(https://github.com/cool-japan/oxisound): `oxisound`, `oxisound-core`,
`oxisound-cpal`, `oxisound-midi`, `oxisound-smf`, `oxisound-osc`,
`oxisound-jack`, and `oxisound-session`. Vulnerabilities in upstream
dependencies (including OS-boundary audio backends such as ALSA,
CoreAudio, WASAPI, or libjack2) should be reported to those projects
directly, though we welcome a heads-up so we can track and update our
dependency pins.

## Maintainer

COOLJAPAN OU (Team Kitasan)
