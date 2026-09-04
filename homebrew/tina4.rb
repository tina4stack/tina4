class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MIT"
  version "3.8.79"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.79/tina4-darwin-arm64"
      sha256 "4f8c9418a731197e3f3c75ae3fe0441192497b31da07f27c2c325002ea71d77a"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.79/tina4-darwin-amd64"
      sha256 "e8bb43f4ca186fb6ded971090f9c15b61e190544827a2e3d16b565c8a162303f"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.79/tina4-linux-arm64"
      sha256 "fdae15219d90d3cb9f40aeb27038fcc06003ba9b24f80b32a8f913ad5cb4c9b1"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.79/tina4-linux-amd64"
      sha256 "fddb19ea6b83bcc1d163ea09fa8f80a1a5b7311a4a33af456c4ba81d03e331a1"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
