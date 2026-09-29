# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MPL-2.0"
  version "3.8.94"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.94/tina4-darwin-arm64"
      sha256 "7cb74f9594b3cd0288d6c5d9a88576442bc142393d9df70fe235c314894c9321"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.94/tina4-darwin-amd64"
      sha256 "4c5577ec9357a9f075317dce58561b952022171fd9389c1595a5198a99968925"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.94/tina4-linux-arm64"
      sha256 "7b44924585373333a8dbcba10d5f5247a43347c9ab9eedba9e50e79a62353940"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.94/tina4-linux-amd64"
      sha256 "fd7c152b5b067c7affc22d5c2ec43d04655c037e70f5d20dfdf0c5916638fd4f"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
