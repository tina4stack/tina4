class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MIT"
  version "3.0.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.0.0/tina4-darwin-arm64"
      sha256 "f78d955eb951f7c2b439f375f182e301093e306b11127ba15c74364c7db1cbb6"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.0.0/tina4-darwin-amd64"
      sha256 "780f47bed8e3d9669810e29c9e43f3aa079ea8dcca330b848c4aca997fb31a49"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.0.0/tina4-linux-arm64"
      sha256 "2afdcd5a87522ae12a9b0852e43b618219fb3b8a3c45935487e5472f67118b3b"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.0.0/tina4-linux-amd64"
      sha256 "b434f035583e411fae8adc614a5a2ec98ada885ebdb6934a14fc7a453989a234"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
