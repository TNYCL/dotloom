# External plugin example

This project is **not** part of the pnpm workspace. It is installed from the packed
`@dotloom/sdk` and `@dotloom/react` tarballs into a temporary directory outside the
repository by `scripts/smoke-packages.mjs`, then built and tested. It may only use
public package entry points — no workspace aliases, no engine internals.

It defines an entity type (`acme.table`: a table whose width follows its seat
count by an engine rule), a tool that places tables, and a constraint template.

```sh
node scripts/smoke-packages.mjs   # from the repository root
```
