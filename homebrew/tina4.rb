class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MIT"
  version "3.8.78"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.78/tina4-darwin-arm64"
      sha256 "b7f401ffa77d335b5ef496c12f4dca4240462d354e9bf255b250dd574fe8a0e5"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.78/tina4-darwin-amd64"
      sha256 "84de1d70665b92ac4fb07ded2f75d092a3566dbc86a743ef48edeac89b1afd42"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.78/tina4-linux-arm64"
      sha256 "5ade4c8a379a535ccea54e6b08b3184822c46189a111f329019e35a0566162ef"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.78/tina4-linux-amd64"
      sha256 "fd564d80315830d6ee04d9ebd69ef0aa2cecd0885b55ecaa96e85075f9f8b82d"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
