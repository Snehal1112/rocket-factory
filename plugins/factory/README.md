# factory plugin

This plugin adds the `/factory` command to Claude Code. The command brainstorms and plans a feature with you, then hands it to the rocket-factory driver, which builds, checks, reviews and merges it.

The plugin does not install the `factory` binary. The `factory` binary must be on your PATH (or set in `$FACTORY_BIN`). Build and install it as described in `docs/guides/getting-started.md`.

## Install

```bash
claude plugin marketplace add Snehal1112/rocket-factory
claude plugin install factory@rocket-factory
```

## Update

```bash
claude plugin update factory@rocket-factory
```

## Generated file

`commands/factory.md` and `.claude-plugin/plugin.json` are generated from the same text `factory install` writes. Do not edit them by hand. After a change, run `factory plugin sync` from the repository root. `factory plugin sync --check` fails when they differ.
