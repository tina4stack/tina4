# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MPL-2.0"
  version "3.8.101"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.101/tina4-darwin-arm64"
      sha256 "57da5b77ed4fc07effc6d74d407b74d0624bacf905fd3113f4fefef0a2d10790"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.101/tina4-darwin-amd64"
      sha256 "386d1b5f60461522f40973223105ed10fe903d5e1a57e118da0e49896af3f772"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.101/tina4-linux-arm64"
      sha256 "a1c3a6702e9597bdf13315dfa397bafee802fb7b8ea651daed7cc30598250520"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.101/tina4-linux-amd64"
      sha256 "faaa4694ff3ea124b97b544714f38496894ddd9f1e3aa86cf9260b4a34e08125"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
