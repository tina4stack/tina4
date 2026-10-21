# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MPL-2.0"
  version "3.8.103"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.103/tina4-darwin-arm64"
      sha256 "771c65c6a885167cc253e3330ab124aba8ea4fc5b0bebe849ce9fb6e9ef36d6d"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.103/tina4-darwin-amd64"
      sha256 "a6f27de8a94cb23e4bbe1558fb89905eb09c0b5bfe0bb3e4e6fdca304c9720da"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.103/tina4-linux-arm64"
      sha256 "cebc2bc70f47ea6d710cf40a21b4be7562dc50cef5cd7d92a609a90917c254ad"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.103/tina4-linux-amd64"
      sha256 "f789fe3e660b5c82e50afa2a3ea619dcebcabfa1fa785978d547e52a13aa1409"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
