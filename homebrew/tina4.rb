class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MIT"
  version "3.8.77"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.77/tina4-darwin-arm64"
      sha256 "1164ef1ee515d4f25266a728f2cb85bb645ee6de576e280019fb2bcdb941466f"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.77/tina4-darwin-amd64"
      sha256 "0240251ae19983602fdcbe1ca55bc9e8ee66ee4d0ea3c6f2907667723ce12a4d"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.77/tina4-linux-arm64"
      sha256 "bf21b446fe9be5ae06448dba25553a70f84a6d62b2503e3483251dfc72657fbf"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.77/tina4-linux-amd64"
      sha256 "0eff60b481a1a5fee184ab12cc21688dafed039db9960b0c3fb1f4c37fb80df4"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
