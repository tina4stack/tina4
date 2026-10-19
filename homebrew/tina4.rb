# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MPL-2.0"
  version "3.8.102"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.102/tina4-darwin-arm64"
      sha256 "bb76d60bd7ee34c5bfceec71a0d0a60d84c898a1a61eab1d7cb56b4ca742f01b"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.102/tina4-darwin-amd64"
      sha256 "2e4fd944b0e33bd434f15c658b84d644d2bb49f5f5f4976dc872110dd7d1fafc"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.102/tina4-linux-arm64"
      sha256 "93b6ff33f033380fc2b1804f22694e0779b873254cd6c042aca9bf8b9a3d98f0"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.102/tina4-linux-amd64"
      sha256 "e33118aaa4e0c113e5c697050aeca2a00fc4f4cca95f4756e71b43a915e603f8"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
