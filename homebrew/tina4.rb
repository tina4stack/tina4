# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MPL-2.0"
  version "3.8.108"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.108/tina4-darwin-arm64"
      sha256 "d7679851925646809f54d3c85b13302fdf665e212345751f713833ca2838e2ee"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.108/tina4-darwin-amd64"
      sha256 "b750ccbc7c6402f890e91b09f3003c36fc89d548aea4a9aa84bbf03063a604af"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.108/tina4-linux-arm64"
      sha256 "0536c9647e983219677b6401688f7147aac90acee1891d687fb5f44afc2eeaa1"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.108/tina4-linux-amd64"
      sha256 "1cf1aea2a266cdc998b0819c4c5a68658e22f5cc8e133a40aa12a53da6dcc79d"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
