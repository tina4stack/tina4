# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MPL-2.0"
  version "3.8.99"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.99/tina4-darwin-arm64"
      sha256 "9ef019b725af541f85c60182e5355030f1e240495f0c8b13b5bc26cc7d89034a"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.99/tina4-darwin-amd64"
      sha256 "3842cf20ecdb678448eadcc1afe197382c51481bf4f57e6cc330b8c5e34c8108"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.99/tina4-linux-arm64"
      sha256 "9788a6244e3f7826ac58b9daa2b3b3bd768f2518be87d97350856a3f11db6100"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.99/tina4-linux-amd64"
      sha256 "7ad94a9f13d415c68037e41d43c958b971f3cb1e25281041c14708a93b8405bc"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
