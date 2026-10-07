# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MPL-2.0"
  version "3.8.97"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.97/tina4-darwin-arm64"
      sha256 "ca66f8a9ce0367d74dfaeea6b57f663269bf69268e56841f38ed89a1336ccd97"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.97/tina4-darwin-amd64"
      sha256 "754194ee6ccf0d7b0fc3e8e732b41b295d94aef1dfc8040dc2e1512a10a8b927"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.97/tina4-linux-arm64"
      sha256 "dddced62c1b3d644bf017afaf769b5259bad78d3e3193e184cb78d6f02f5cb7a"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.97/tina4-linux-amd64"
      sha256 "bf48d836efcee58aebe02eafbb4c9da0c1bc41369a88cd35287dc7199e41b0a1"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
