class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MIT"
  version "3.8.87"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.87/tina4-darwin-arm64"
      sha256 "d51aba54b771accc5710d07602927a9a22620e39fc3d94ba4ab8a2dc006a6e40"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.87/tina4-darwin-amd64"
      sha256 "1a8092721967ae3b062cf63f837ea24acaf3089d3762c13ab844c3afaa8aea50"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.87/tina4-linux-arm64"
      sha256 "625e14438f11ad16144cfbd44f4ee6d6d2a4e59d47636eae50f56751d2251cc9"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.87/tina4-linux-amd64"
      sha256 "a8f2e10181dbf4d21dc2212c93a7fab67b780adebb33e70b2dd76dc0c18772d0"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
