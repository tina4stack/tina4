class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MIT"
  version "3.8.85"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.85/tina4-darwin-arm64"
      sha256 "5ec65110f70c358d46979ea847c9cdb1ab42ee50a3c7144c3397ea305c7a0aeb"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.85/tina4-darwin-amd64"
      sha256 "a290b4cea0595677d324875332f8c2a4ab04ba22dbfe7de8198e1846063d5f21"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.85/tina4-linux-arm64"
      sha256 "2bb8606fd955b3f38d8e9d283d88b01b4709d1392b28ba9c178eed0d4d54abbf"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.85/tina4-linux-amd64"
      sha256 "9e7c06a486385b7a8c9d33ad4eb76d24f6ce46f2ee603d0b0e55890f2e2feaea"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
