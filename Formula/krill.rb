# Homebrew formula — the main repo doubles as the tap (it has Formula/):
#   brew tap elpalaiso/krill https://github.com/elpalaiso/krill
#   brew install --HEAD elpalaiso/krill/krill   # until the first tagged release
#
# After the first `git tag v0.1.0` release: fill in sha256 from the
# release job's .sha256 asset and drop the --HEAD requirement.
class Krill < Formula
  desc "Tiny orchestrator for AI coding agents - tmux + git worktrees"
  homepage "https://github.com/elpalaiso/krill"
  url "https://github.com/elpalaiso/krill/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "" # TODO: set on the first tagged release
  license "MIT"
  head "https://github.com/elpalaiso/krill.git", branch: "main"

  depends_on "rust" => :build
  depends_on "tmux"

  def install
    system "cargo", "install", *std_cargo_args(path: "crates/krill")
  end

  test do
    assert_match "krill", shell_output("#{bin}/krill --version")
  end
end
