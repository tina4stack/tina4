# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MPL-2.0"
  version "3.8.106"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.106/tina4-darwin-arm64"
      sha256 "555c654685a633946a2224728dd5f88610e61019303b2c7a02dfc65ac88a192d"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.106/tina4-darwin-amd64"
      sha256 "00c11e3ec73bcb47e1ea6dcab5cf9963b80a41886c2eee2d49691cc92eb95e71"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.106/tina4-linux-arm64"
      sha256 "546845014ac2e46c01411d75513ef05a0a4e079f596d3bcd81fe1e1bf9116c21"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.106/tina4-linux-amd64"
      sha256 "50601954615173584df1c3fb4c15c3737c0668e80d7dc5463771eeb315bf7e40"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
