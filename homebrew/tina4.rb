# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MPL-2.0"
  version "3.8.113"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.113/tina4-darwin-arm64"
      sha256 "8233be21d8ffd5741e9a6719de8e0131d0632b5762a001d71a1ad4738d11f4c1"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.113/tina4-darwin-amd64"
      sha256 "bac73470e9b6bbc2324db34a82cb579cd50eef1c7ce63efae0c4ef0983cddf2a"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.113/tina4-linux-arm64"
      sha256 "0d2fd14ce0ace6f02c0b1ae8afc74e4ca07174aa049ec3097c1df33c9aad4182"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.113/tina4-linux-amd64"
      sha256 "7dfe39e7eddcaf147e24a7b94bba7c57cb30a47eda8a9699f14b55dac46f94e2"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
