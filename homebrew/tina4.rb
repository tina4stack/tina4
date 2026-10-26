# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MPL-2.0"
  version "3.8.107"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.107/tina4-darwin-arm64"
      sha256 "ec2e06ab1391aadf137b965cf692cc0bb39415881f97f7d02e2dea2bd7fe5206"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.107/tina4-darwin-amd64"
      sha256 "5c5ded4c11f8a8ca7366eabc7e8001006de5929aaf83f2358273062573abab66"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.107/tina4-linux-arm64"
      sha256 "7bc4c7d55b208b7c20ad0936eb28c1128a15846d029895a6ba2d8789b04cce96"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.107/tina4-linux-amd64"
      sha256 "e20682baa041012c2140fc09fa9843a53d615ed9e89fee4b8828f91a3f2ef8ba"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
