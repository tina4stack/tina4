class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MIT"
  version "3.8.84"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.84/tina4-darwin-arm64"
      sha256 "b222b4c2a6b5dae4cc94d9aa1cfa23d75aa119a92f95755aaadfa52d7886d24e"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.84/tina4-darwin-amd64"
      sha256 "a4aac2fb8a3c9bfec7ee06bb732d0a50a7c81edf812205d611e1ddec34bc7309"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.84/tina4-linux-arm64"
      sha256 "1f55c32993c680f5a837438c6b3c6f79da4092284b4630901ae45cd635d54f15"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.84/tina4-linux-amd64"
      sha256 "c01dc9fa71c47b0573a868f1af915d6e217eb577b3128f52946a0917bd512656"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
