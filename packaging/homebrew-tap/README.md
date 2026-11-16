# Homebrew tap for HardViz

```sh
brew tap liorbass/tap
brew install --cask liorbass/tap/hardviz
```

HardViz is a hardware monitoring dashboard: a windowed app and a terminal UI
reading the same sensors — CPU, GPU, memory, temperatures, fans, network and
disks. This tap installs the build from the GitHub release archive.

The cask here is a copy. It is authored in the main repository, at
[`packaging/homebrew-tap/Casks/hardviz.rb`](https://github.com/liorbass/hardviz/blob/main/packaging/homebrew-tap/Casks/hardviz.rb),
and a script re-pins its version and hash from the published asset after every
release, so this file is only ever updated by copying it across.

Two things to know:

- **Apple Silicon only.** Releases carry no `x86_64` macOS archive, so the cask
  has one hash and will not install on an Intel Mac.
- **Not signed or notarised.** Homebrew fetches with curl, which does not set
  the quarantine attribute that triggers Gatekeeper, so this normally does not
  matter. When it does, the cask says what to do.

Issues with HardViz itself belong in
[the main repository](https://github.com/liorbass/hardviz/issues), not here.
