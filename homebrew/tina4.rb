class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MIT"
  version "3.8.86"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.86/tina4-darwin-arm64"
      sha256 "d3230b7a8011acecb73f7d8d989b6cbd2ad0a262bfc20386bdacf0988711d7f8"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.86/tina4-darwin-amd64"
      sha256 "dc7a6a710c30e6c3d52aaea8a97caf6664781e139384d944868e5001362bef6d"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.86/tina4-linux-arm64"
      sha256 "e2240db8e6596d1a9e35a7ee625b8f8991973a9e51a6c59e68950aa5e17fe81a"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.86/tina4-linux-amd64"
      sha256 "3182b96cd020664142d0dead636962ca7c9ec3e71880266f6a1811db61582e3a"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
