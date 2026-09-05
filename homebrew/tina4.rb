class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MIT"
  version "3.8.81"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.81/tina4-darwin-arm64"
      sha256 "fb80141ff085222c6170374857ab2aeecb9d9163a450bc016ab97b938b9a5ec4"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.81/tina4-darwin-amd64"
      sha256 "8cca08fb65ca8693d8f13da5b4cdb2e498accdc27baee6475106a2394acdf015"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.81/tina4-linux-arm64"
      sha256 "eecf67dddc6eb8949c3c34480512d3701a3645213910485df51995893d02a0a2"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.81/tina4-linux-amd64"
      sha256 "dc6508ae932de2d16a435cbd53a4729cce33289bafb5484a8d7aed9e267876c9"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
