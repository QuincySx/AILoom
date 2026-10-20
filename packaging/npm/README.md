# ailoom-cli

Command-line tool for [AILoom](https://github.com/QuincySx/AILoom): manage skills, rules and agents for AI coding tools (Claude Code, Codex and others) across your projects.

```bash
npm install -g ailoom-cli
ailoom web        # start the local web console and open it
```

The package contains prebuilt binaries for every supported platform and runs the one for your system; nothing is downloaded at run time. Supported platforms and offline use: [PLATFORMS.md](PLATFORMS.md). Full guide: [installation docs](https://github.com/QuincySx/AILoom/blob/main/docs/guide/INSTALL.md).

## Install the current checkout for local testing

On macOS / Linux, with Rust and Node.js installed, run from the repository root:

```sh
npm --prefix packaging/npm run install:local
ailoom-test web
```

The script builds release binaries, packs and installs a private native-platform npm package, then verifies the installed binary and wrapper. It installs into `~/.local/opt/ailoom-test/builds/` and updates `~/.local/bin/ailoom-test`. Each installation is preserved; a failed build or package verification leaves the previous entry usable. Your existing `ailoom` installation is retained.

An already running web service switches to the verified package on the same port. Refresh the browser afterward. Login startup keeps its original entry; use `ailoom-test service enable` if you also want the test build at login. Run the same command after code changes. To install without connecting to or restarting your web service:

```sh
npm --prefix packaging/npm run install:local -- --no-restart
```

Use `--prefix <path>` and `--bin-dir <path>` for a separate test installation. If `~/.local/bin` is absent from your PATH, use the full entry path printed by the installer. This developer script is available in the source checkout; it is excluded from published packages.

License: Apache-2.0
