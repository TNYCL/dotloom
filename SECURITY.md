# Security policy

## Reporting a vulnerability

Please report vulnerabilities **privately** through GitHub:
[Report a vulnerability](https://github.com/TNYCL/dotloom/security/advisories/new)
(Security tab → "Report a vulnerability"). Do not open a public issue.

Include what is affected (crate, package, file format, version or commit), how to
reproduce it (a minimal file or snippet), and the impact you expect. You will get
an acknowledgement within 7 days. Dotloom is maintained by one person; fixes are
prioritized by severity, and you will be credited in the advisory unless you prefer
otherwise.

## Supported versions

Dotloom is pre-release (`0.1.x`). Security fixes go to the latest `main` and the
next release; there are no maintained older branches yet.

## Scope

In scope:

- parsing untrusted input: `.dotl` containers (ZIP, JSON, assets), document JSON,
  SVG and DXF import, scene buffers, plugin type definitions and their expression
  language, worker protocol messages;
- the browser packages (`@dotloomjs/sdk`, `@dotloomjs/react`): anything that lets a
  document or an imported file run script, inject markup or fetch from the network;
- resource exhaustion that bypasses the documented limits (sizes, counts, nesting,
  solver budgets);
- the repository's GitHub Actions workflows.

Design guarantees you can rely on (and report if broken):

- `.dotl` files never execute code and never trigger network requests;
- SVG import never follows external references and inserts nothing into the page;
  `script`, `foreignObject`, `use` and external `image` content is skipped and
  reported;
- every parser has explicit limits and returns errors instead of panicking (fuzzed
  nightly, corpus replayed in every CI run).

Out of scope: plugins you register yourself run with your application's
privileges (their *definitions* are data and in scope; your own tool or panel code
is not); vulnerabilities in browsers, GPU drivers or third-party dependencies
(please report those upstream — `cargo deny` checks advisories nightly).
