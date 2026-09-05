class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MIT"
  version "3.8.83"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.83/tina4-darwin-arm64"
      sha256 "98199889dd6b906da5a75f35ce6af5d0af981df95c67e8b298ba0af06501c91a"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.83/tina4-darwin-amd64"
      sha256 "67b43f33386cfaad8a40b015d40dca0a606c778a43a45064507090fb664c1861"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.83/tina4-linux-arm64"
      sha256 "6913bd47d11f42fb4a9883263dab38c2f3c737c4f8e33105e5789fa057a0384e"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.83/tina4-linux-amd64"
      sha256 "651cc398dcc20f070c14b109f6d7c4ec7b9453891ba18579ec15747d56bb04f4"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
