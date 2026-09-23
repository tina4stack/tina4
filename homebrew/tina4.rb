class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MIT"
  version "3.8.89"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.89/tina4-darwin-arm64"
      sha256 "c9001889048fbf0406678d9c7197a4ff6f1397aec5c9264d61e9be50b51294b2"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.89/tina4-darwin-amd64"
      sha256 "426e7127dc437cd04f1c7d80d0fa67bdffd8287c0911e46f41459c54a4944dba"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.89/tina4-linux-arm64"
      sha256 "d665753086e8ae82e0327fe6ebff6bfde1a76dacc94e7fb7c1087ca1954eb11b"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.89/tina4-linux-amd64"
      sha256 "16521cbb1fcedc4230ba50cf6c380e704b0990ffa8b081bd990ac33fded16162"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
