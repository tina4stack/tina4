class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MIT"
  version "3.8.82"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.82/tina4-darwin-arm64"
      sha256 "b864d6beecd2d1dd011709155ed9d5eede18a06ceca592236bbe13f53e72a0eb"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.82/tina4-darwin-amd64"
      sha256 "f4c05df0e0ac1eb61df7bac4e272ecf41a889bce3af16e91f9266228895f42a7"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.82/tina4-linux-arm64"
      sha256 "4c69e747e510ab39ddfc48efa9247e9a9e34c6aa7bb5e80f638468cefe575d98"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.82/tina4-linux-amd64"
      sha256 "a6544867fb35947f0e4e88031454dbbfefe63be7d1827bcb56c0aad008c23f28"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
