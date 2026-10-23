# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MPL-2.0"
  version "3.8.104"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.104/tina4-darwin-arm64"
      sha256 "f5208867befee450fbdfa7bc934ff55fd16b92857ab5d9baa38a6aecca0991cb"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.104/tina4-darwin-amd64"
      sha256 "e6e1422f4546a707e5351a3d69100148121544fb14b160c02058f7f83a94419a"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.104/tina4-linux-arm64"
      sha256 "be43e2bcc23ad57f35597fc91f99b9dec244c2573c942bb378a7074c13c3c059"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.104/tina4-linux-amd64"
      sha256 "cbc106d57ee515a794eb5d87874167463383e81501b7cdf8d0dd2d72dfd07822"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
