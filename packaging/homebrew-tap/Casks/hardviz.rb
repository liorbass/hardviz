cask "hardviz" do
  version "0.1.5"
  # Apple Silicon only: the releases carry no x86_64 macOS asset.
  sha256 "3138c464eef61cf06435d9aa0f8fc30aa48243e618d8a7a2d2593905524f261c"

  url "https://github.com/liorbass/hardviz/releases/download/v#{version}/hardviz-macos-arm64.tar.gz"
  name "HardViz"
  desc "Modular hardware monitoring dashboard for CPU, GPU, memory, temperatures, network and disks"
  homepage "https://github.com/liorbass/hardviz"

  livecheck do
    url "https://github.com/liorbass/hardviz/releases/latest"
    strategy :github_latest
  end

  # The bare symbol means ">= :big_sur" (MacOSRequirement.parse passes the
  # comparator through); the string form now warns on every brew command that
  # loads this file, in front of the user, asking them to report it to this tap.
  # When Homebrew drops Big Sur support (their note says September 2027 or
  # later), the symbol moves to DISABLED_MACOS_VERSIONS and this line has to go.
  depends_on macos: :big_sur

  app "hardviz-macos-arm64/HardViz.app"
  binary "hardviz-macos-arm64/hardviz-tui"

  # Until SignPath signing and Apple notarisation land. Homebrew fetches with
  # curl, which does not set com.apple.quarantine, so Gatekeeper normally stays
  # out of the way; this covers the case where it does not.
  caveats "HardViz is not signed or notarised yet. If macOS refuses to open it, right-click HardViz.app and choose Open once."
end
