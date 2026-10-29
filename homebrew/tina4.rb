# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MPL-2.0"
  version "3.8.112"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.112/tina4-darwin-arm64"
      sha256 "a7cafa970609153556b6c4995dacbbb83598a51a924918032d52e580b608e94a"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.112/tina4-darwin-amd64"
      sha256 "6defaf4813dedb83ac2ba32affa5c2d292ca95fe239b3468e00a5dd7186c314e"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.112/tina4-linux-arm64"
      sha256 "01ed6bc717436567daadfd0ad5bf043fc20a2bd1c3e80ea9c5933ab53ad248d2"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.112/tina4-linux-amd64"
      sha256 "e7f88777a2331616da8c6691d3da11b7e43f00071b6d30d86d687159ab89d6a0"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
